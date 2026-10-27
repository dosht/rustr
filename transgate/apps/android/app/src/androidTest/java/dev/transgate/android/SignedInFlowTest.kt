package dev.transgate.android

import android.content.Intent
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.UiObject2
import androidx.test.uiautomator.Until
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.util.regex.Pattern

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
        context.startActivity(
            context.packageManager.getLaunchIntentForPackage(context.packageName)!!.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TASK or Intent.FLAG_ACTIVITY_NEW_TASK),
        )
        // Either the sign-in screen, or a session a previous run left behind: sign that out through
        // the app's own drawer so every test starts at sign-in with the Keystore session cleared.
        val first = device.wait(Until.findObject(By.res(Pattern.compile("auth-email|menu"))), SIGN_IN_MS)
            ?: error("the app showed neither sign-in nor the signed-in frame")
        if (first.resourceName == "menu") {
            first.click()
            click(By.res("nav-sign-out"))
            wait(By.res("auth-email"))
        }
    }

    @After
    fun leave() {
        device.pressHome()
    }

    @Test
    fun signInShowsTheSignedInFrameAndBalance() {
        signIn()
        click(By.res("menu"))
        assertTrue("the drawer shows the balance core reports", wait(By.res("balance")).text.isNotBlank())
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
        click(By.res("nav-sign-out"))
        wait(By.res("auth-email"))
    }

    private fun signIn() {
        wait(By.res("auth-email")).text = email
        wait(By.res("auth-password")).text = password
        click(By.res("auth-submit"))
        wait(By.res("menu"), SIGN_IN_MS)
    }

    private fun openNewestJob() {
        click(By.res("menu"))
        click(By.res("nav-all"))
        wait(By.res("jobs-list"))
        click(By.res(Pattern.compile("job-row-\\d+")))
    }

    private fun click(selector: BySelector) = wait(selector).click()

    private fun wait(selector: BySelector, timeoutMs: Long = STEP_MS): UiObject2 =
        device.wait(Until.findObject(selector), timeoutMs) ?: error("not on screen within ${timeoutMs}ms: $selector")

    private companion object {
        const val STEP_MS = 15_000L
        const val SIGN_IN_MS = 45_000L
    }
}
