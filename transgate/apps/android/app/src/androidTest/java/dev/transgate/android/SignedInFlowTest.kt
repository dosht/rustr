package dev.transgate.android

import android.content.Intent
import android.net.Uri
import android.util.Base64
import android.util.Log
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.Direction
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.UiObject2
import androidx.test.uiautomator.Until
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TestWatcher
import org.junit.runner.Description
import java.io.File
import java.util.regex.Pattern
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.UUID
import org.json.JSONObject
import dev.transgate.android.ports.KeystoreVault
import dev.transgate.core.HomeView
import dev.transgate.core.Route
import dev.transgate.core.webUrl

/**
 * D14 acceptance: the signed-in flow, driven from outside the app by UiAutomator on a device
 * (Waydroid). It runs against a live host (`transgate.apiUrl`/`authUrl` baked into the build)
 * whose account's newest job is COMPLETED and has transcript lines.
 *
 *   adb shell am instrument -w \
 *     -e email ada@example.com -e password '<password>' \
 *     dev.transgate.android.test/androidx.test.runner.AndroidJUnitRunner
 *
 * The screens expose their Compose test tags as resource ids, so every step names an element
 * the way the app does. Nothing here knows a business rule: it types, taps and reads what the
 * core's view models put on screen.
 */
class SignedInFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val arguments = InstrumentationRegistry.getArguments()
    private val email = arguments.getString("email") ?: error("pass -e email <account email>")
    private val password = arguments.getString("password") ?: error("pass -e password <account password>")

    @Before
    fun launchSignedOut() {
        val context = instrumentation.targetContext
        device.pressHome()
        instrumentation.startActivitySync(
            context.packageManager.getLaunchIntentForPackage(context.packageName)!!.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TASK or Intent.FLAG_ACTIVITY_NEW_TASK),
        )
        instrumentation.waitForIdleSync()
        device.waitForIdle()
        // Either the sign-in screen, or a session a previous run left behind: sign that out through
        // the app's own drawer so every test starts at sign-in with the Keystore session cleared.
        val first = device.wait(Until.findObject(By.res(Pattern.compile("auth-email|menu"))), SIGN_IN_MS)
            ?: error("the app showed neither sign-in nor the signed-in frame")
        if (first.resourceName == "menu") {
            click(By.res("menu"))
            clickDrawer(By.res("nav-sign-out"))
            wait(By.res("auth-email"))
        }
    }

    @get:Rule
    val failureCapture = object : TestWatcher() {
        override fun failed(error: Throwable, description: Description) {
            val directory = File(instrumentation.targetContext.getExternalFilesDir(null), "acceptance-failures").apply { mkdirs() }
            val prefix = "${description.methodName}-${System.currentTimeMillis()}"
            runCatching { device.takeScreenshot(File(directory, "$prefix.png")) }
                .onFailure { Log.e("TransgateAcceptance", "Failure screenshot could not be captured", it) }
            runCatching { device.dumpWindowHierarchy(File(directory, "$prefix.xml")) }
                .onFailure { Log.e("TransgateAcceptance", "Failure hierarchy could not be captured", it) }
            Log.i("TransgateAcceptance", "Failure evidence: ${File(directory, prefix)}")
        }

        override fun finished(description: Description) {
            device.pressHome()
        }
    }

    @Test
    fun warmAuthReturnReportsAnInvalidCode() {
        openAuthReturn(clearTask = false)
        assertTrue(wait(By.res("auth-error")).text.isNotBlank())
    }

    @Test
    fun coldAuthReturnReportsAnInvalidCode() {
        openAuthReturn(clearTask = true)
        assertTrue(wait(By.res("auth-error")).text.isNotBlank())
    }

    @Test
    fun warmAuthReturnExchangesPkceAndRestoresSession() = validAuthReturn(clearTask = false)

    @Test
    fun coldAuthReturnExchangesPkceAndRestoresSession() = validAuthReturn(clearTask = true)

    private fun validAuthReturn(clearTask: Boolean) {
        val fixture = arguments.getString("pkceFixture") ?: error("pass -e pkceFixture <local auth proxy origin>")
        val verifier = UUID.randomUUID().toString() + UUID.randomUUID().toString()
        val challenge = Base64.encodeToString(MessageDigest.getInstance("SHA-256").digest(verifier.toByteArray()), Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING)
        val issued = fixtureRequest("$fixture/__android/pkce", JSONObject().put("email", email).put("password", password).put("challenge", challenge))
        val code = issued.getString("code")
        val vault = KeystoreVault(instrumentation.targetContext)
        vault.set("transgate.session.code-verifier", verifier)
        openAuthReturn(clearTask, code)
        wait(By.res("menu"), SIGN_IN_MS)
        assertNull("the successful exchange consumed its verifier", vault.get("transgate.session.code-verifier"))
        val session = JSONObject(vault.get("transgate.session") ?: error("session was not persisted"))
        assertEquals(email, session.getString("email"))
        assertTrue(session.getString("access_token").isNotBlank())
        assertEquals(1, fixtureRequest("$fixture/__android/pkce/$code").getInt("exchanges"))

        val context = instrumentation.targetContext
        instrumentation.startActivitySync(context.packageManager.getLaunchIntentForPackage(context.packageName)!!.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TASK or Intent.FLAG_ACTIVITY_NEW_TASK))
        instrumentation.waitForIdleSync()
        device.waitForIdle()
        wait(By.res("menu"), SIGN_IN_MS)
        click(By.res("menu"))
        assertTrue(wait(By.res("balance")).text.isNotBlank())
        assertEquals(email, JSONObject(vault.get("transgate.session")!!).getString("email"))
        assertEquals("restoring the stored session must not exchange the callback again", 1, fixtureRequest("$fixture/__android/pkce/$code").getInt("exchanges"))
    }

    private fun fixtureRequest(url: String, body: JSONObject? = null): JSONObject {
        val connection = URL(url).openConnection() as HttpURLConnection
        connection.connectTimeout = STEP_MS.toInt()
        connection.readTimeout = SIGN_IN_MS.toInt()
        try {
            if (body != null) {
                connection.requestMethod = "POST"
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/json")
                connection.outputStream.use { it.write(body.toString().toByteArray()) }
            }
            assertEquals("local fixture request succeeded", 200, connection.responseCode)
            return JSONObject(connection.inputStream.bufferedReader().use { it.readText() })
        } finally {
            connection.disconnect()
        }
    }

    @Test
    fun signInShowsTheSignedInFrameAndBalance() {
        signIn()
        click(By.res("menu"))
        assertTrue("the drawer shows the balance core reports", wait(By.res("balance")).text.isNotBlank())
    }

    @Test
    fun settingsOpenAndCloseReturnsToTheSignedInFrame() {
        signIn()
        click(By.res("menu"))
        clickDrawer(By.res("nav-settings"))
        wait(By.text("Settings"))
        wait(By.text(email))
        click(By.desc("Close"))
        assertTrue("closing settings removes its dialog", device.wait(Until.gone(By.desc("Close")), STEP_MS))
        wait(By.res("menu"))
        click(By.res("menu"))
        clickDrawer(By.res("nav-settings"))
        wait(By.text(email))
        click(By.desc("Close"))
        assertTrue("reopened settings can also be dismissed", device.wait(Until.gone(By.desc("Close")), STEP_MS))
        wait(By.res("menu"))
    }

    @Test
    fun editingALineSavesAndSurvivesLeavingTheTranscript() {
        signIn()
        openNewestJob()
        val line = wait(By.res(Pattern.compile("line-text-\\d+")))
        val id = line.resourceName.substringAfterLast('-')
        val original = line.text

        click(By.res("line-edit-$id"))
        val editor = wait(By.res("line-editor"))
        assertEquals("the editor starts from the stored line text", original, editor.text)

        val edited = "uia-edit-${System.currentTimeMillis()}"
        editor.text = edited
        click(By.res("line-save"))
        assertEquals("the saved text comes back through the view model", edited, wait(By.res("line-text-$id")).text)

        // Leave the transcript and open it again: the line is read back from core, not kept by the screen.
        device.pressBack()
        wait(By.res("jobs-list"))
        click(By.res(Pattern.compile("job-row-\\d+")))
        assertEquals("the edit persisted on the host", edited, wait(By.res("line-text-$id")).text)

        // Put the original text back so the account is left as it was found.
        click(By.res("line-edit-$id"))
        wait(By.res("line-editor")).text = original
        click(By.res("line-save"))
        assertEquals(original, wait(By.res("line-text-$id")).text)
    }

    @Test
    fun cancellingALineEditKeepsTheStoredText() {
        signIn()
        openNewestJob()
        val line = wait(By.res(Pattern.compile("line-text-\\d+")))
        val id = line.resourceName.substringAfterLast('-')
        val original = line.text

        click(By.res("line-edit-$id"))
        wait(By.res("line-editor")).text = "never saved"
        click(By.res("line-cancel"))
        assertEquals(original, wait(By.res("line-text-$id")).text)
    }

    @Test
    fun signOutReturnsToTheSignInScreen() {
        signIn()
        click(By.res("menu"))
        clickDrawer(By.res("nav-sign-out"))
        wait(By.res("auth-email"))
    }

    private fun signIn() {
        wait(By.res("auth-email")).text = email
        wait(By.res("auth-password")).text = password
        click(By.res("auth-submit").enabled(true))
        wait(By.res("menu"), SIGN_IN_MS)
    }

    private fun openAuthReturn(clearTask: Boolean, code: String = "unissued-android-regression-code") {
        val context = instrumentation.targetContext
        val callback = Uri.parse(webUrl(BuildConfig.SITE_URL, Route.Home(HomeView.Default), "en")).buildUpon()
            .appendQueryParameter("code", code)
            .build()
        val flags = Intent.FLAG_ACTIVITY_NEW_TASK or if (clearTask) Intent.FLAG_ACTIVITY_CLEAR_TASK else 0
        context.startActivity(
            Intent(Intent.ACTION_VIEW, callback, context, MainActivity::class.java).addFlags(flags),
        )
    }

    private fun openNewestJob() {
        click(By.res("menu"))
        click(By.res("nav-all"))
        wait(By.res("jobs-list"))
        click(By.res(Pattern.compile("job-row-\\d+")))
    }

    private fun click(selector: BySelector) = wait(selector).click()

    private fun clickDrawer(selector: BySelector) {
        val drawer = wait(By.res("drawer"))
        drawer.scrollUntil(Direction.DOWN, Until.findObject(selector)) ?: error("drawer item not found: $selector")
        click(selector)
    }

    private fun wait(selector: BySelector, timeoutMs: Long = STEP_MS): UiObject2 =
        device.wait(Until.findObject(selector), timeoutMs) ?: error("not on screen within ${timeoutMs}ms: $selector")

    private companion object {
        const val STEP_MS = 15_000L
        const val SIGN_IN_MS = 45_000L
    }
}
