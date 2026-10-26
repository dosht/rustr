use super::*;

fn route(url: &str) -> Route {
    parse(url).unwrap_or_else(|| panic!("{url} is an app route")).route
}

#[test]
fn home_views_jobs_and_pages_parse_with_or_without_a_locale_prefix() {
    assert_eq!(route("/home"), Route::Home(HomeView::Default));
    assert_eq!(route("/home?view=new"), Route::Home(HomeView::New));
    assert_eq!(route("/de/home?view=all-transcripts"), Route::Home(HomeView::AllTranscripts));
    assert_eq!(route("/home/42"), Route::Home(HomeView::Job(JobId(42))));
    assert_eq!(route("/signin"), Route::SignIn);
    assert_eq!(route("/new-password"), Route::NewPassword);
    assert_eq!(route("/tr/billing"), Route::Billing);
    assert_eq!(route("/checkout/confirm"), Route::CheckoutConfirm);
    assert_eq!(parse("/id/home").unwrap().locale, "id");
    assert_eq!(parse("/home").unwrap().locale, "en");
    assert_eq!(route("/home?view=unknown"), Route::Home(HomeView::Default));
}

#[test]
fn aliases_parse_to_their_canonical_route_and_ask_for_replacement() {
    let all = parse("/home/all-transcripts").unwrap();
    assert_eq!((all.route, all.canonical), (Route::Home(HomeView::AllTranscripts), true));
    let signin = parse("/fr/new-signin").unwrap();
    assert_eq!((signin.route, signin.locale, signin.canonical), (Route::SignIn, "fr", true));
    assert!(!parse("/home?view=all-transcripts").unwrap().canonical);
    let explicit = parse("/en/home/3").unwrap();
    assert_eq!((explicit.route, explicit.locale, explicit.canonical), (Route::Home(HomeView::Job(JobId(3))), "en", true));
}

#[test]
fn public_api_asset_and_unknown_paths_are_not_app_routes() {
    for path in ["/", "/de", "/en", "/pricing", "/api/jobs", "/api/unknown", "/home/abc", "/dashboard/api-keys", "/checkout", "/favicon.png", "/xx/home"] {
        assert!(parse(path).is_none(), "{path}");
        assert!(!is_app_path(path), "{path}");
    }
    assert!(is_app_path("/home/7?x=1") && is_app_path("/kk/new-password"));
}

#[test]
fn a_callback_code_or_error_is_read_once_and_asks_for_a_clean_location() {
    let ok = parse("/home?code=ab%2Dcd+ef#frag").unwrap();
    assert_eq!((ok.auth_code.as_deref(), ok.canonical), (Some("ab-cd ef"), true));
    let failed = parse("/new-password?error=access_denied&error_description=Email+link+is+invalid").unwrap();
    assert_eq!(failed.auth_error.as_deref(), Some("Email link is invalid"));
    assert_eq!(parse("/home?code=").unwrap().auth_code, None);
    assert_eq!(parse("/home?code=%zz%4").unwrap().auth_code.as_deref(), Some("%zz%4"), "a malformed escape stays literal");
}

#[test]
fn href_formats_the_canonical_path_and_round_trips() {
    let routes = [
        Route::Home(HomeView::Default),
        Route::Home(HomeView::New),
        Route::Home(HomeView::AllTranscripts),
        Route::Home(HomeView::Job(JobId(9))),
        Route::SignIn,
        Route::NewPassword,
        Route::Billing,
        Route::CheckoutConfirm,
    ];
    for r in routes {
        for locale in ["en", "de"] {
            let path = href(r, locale);
            let back = parse(&path).unwrap();
            assert_eq!((back.route, back.locale, back.canonical), (r, locale, false), "{path}");
        }
    }
    assert_eq!(href(Route::Home(HomeView::Job(JobId(9))), "de"), "/de/home/9");
    assert_eq!(href(Route::SignIn, "en"), "/signin");
    assert_eq!(href(Route::SignIn, "xx"), "/signin", "an unknown locale has no prefix");
    assert_eq!(public_root("en"), "/");
    assert_eq!(public_root("az"), "/az");
}

#[test]
fn only_home_and_billing_need_a_signed_in_user() {
    assert!(Route::Home(HomeView::AllTranscripts).protected() && Route::Billing.protected());
    assert!(!Route::SignIn.protected() && !Route::NewPassword.protected() && !Route::CheckoutConfirm.protected());
}
