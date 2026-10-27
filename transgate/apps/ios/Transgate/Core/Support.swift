import Foundation

/// Where the app talks to, from build settings (see `project.yml`). Nothing here is
/// a business rule; core receives these as `PresenterConfig`.
struct AppSettings: Equatable, Sendable {
    /// The API root, e.g. `https://app.transgate.example/api`.
    var apiUrl: String
    /// The GoTrue root, e.g. `https://app.transgate.example/auth/v1`.
    var authUrl: String
    var authKey: String?
    /// The custom-scheme origin auth redirects return to, e.g. `transgate://app`.
    /// GoTrue must allow it as a redirect URL (see README).
    var siteUrl: String
    var captcha = false
    /// The web app's https origin, for the browser handoffs (D6): billing history and API keys.
    /// `nil` hides them.
    var webOrigin: String? = nil

    /// A page of the web app for the system browser (D6): the address is core's `web_url` over
    /// the https web origin (never the auth-callback scheme), in the current locale.
    func webPage(_ route: Route, locale: String) -> URL? {
        guard let base = webOrigin else { return nil }
        return URL(string: webUrl(siteUrl: base, route: route, locale: locale))
    }

    /// The scheme the authentication session returns through.
    var callbackScheme: String { URL(string: siteUrl)?.scheme ?? "transgate" }

    func presenterConfig(userAgent: String) -> PresenterConfig {
        PresenterConfig(apiUrl: apiUrl, authUrl: authUrl, authKey: authKey, siteUrl: siteUrl, captcha: captcha, userAgent: userAgent)
    }

    /// Empty build settings arrive as empty strings; they count as unset, and the app
    /// cannot start without the three required addresses.
    static func fromBundle(_ bundle: Bundle = .main) -> AppSettings? {
        func value(_ key: String) -> String? {
            let text = bundle.object(forInfoDictionaryKey: key) as? String
            return text?.isEmpty == false ? text : nil
        }
        guard let api = value("TransgateApiUrl"), let auth = value("TransgateAuthUrl"), let site = value("TransgateSiteUrl") else { return nil }
        return AppSettings(apiUrl: api, authUrl: auth, authKey: value("TransgateAuthKey"), siteUrl: site, webOrigin: value("TransgateWebUrl"))
    }
}

extension AppVm {
    /// The UI locale core chose; every message is translated in it.
    var locale: String {
        switch self {
        case .restoring(let locale), .checkoutConfirm(let locale, _, _), .erased(let locale, _): return locale
        case .signedOut(let vm): return vm.locale
        case .signedIn(let vm): return vm.locale
        }
    }
}

/// Copy only comes from core's `translate()` (D3): view models carry message keys.
/// A message that cannot be translated is a bug core's tests should have caught;
/// the shell logs it and shows the key rather than inventing text.
struct Copy: Sendable {
    var locale: String

    func text(_ message: Msg) -> String {
        do {
            return try translate(locale: locale, message: message)
        } catch {
            Diagnostics.warn("untranslated message \(locale)/\(message.key): \(error)")
            return message.key
        }
    }

    func text(_ message: Msg?) -> String? { message.map(text) }
}

enum Diagnostics {
    static func warn(_ line: String) {
        FileHandle.standardError.write(Data("transgate: \(line)\n".utf8))
    }
}

/// The custom-scheme return of an authentication (D7): the Google redirect, a
/// magic link, a signup confirmation or a recovery link all arrive as
/// `transgate://app/<locale>/home?code=…`.
enum AuthReturn {
    /// The location core starts from, as the web does on page load: path and query
    /// of the returned URL. `nil` for any other scheme.
    static func startLocation(for url: URL, scheme: String) -> String? {
        guard url.scheme?.lowercased() == scheme.lowercased() else { return nil }
        var location = url.path.isEmpty ? "/home" : url.path
        if let query = url.query, !query.isEmpty { location += "?\(query)" }
        return location
    }
}
