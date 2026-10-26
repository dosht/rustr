/// Converts a serialized HTTP(S) browser `location.href` to core's path + search.
/// Preserve escapes for core to decode once, discard the fragment, and reuse the
/// input buffer rather than allocating another URL. Relative locations pass through.
pub fn app_location(mut href: String) -> String {
    if let Some(fragment) = href.find('#') {
        href.truncate(fragment);
    }
    if let Some(authority) = href.strip_prefix("https://").or_else(|| href.strip_prefix("http://"))
        && let Some(path) = authority.find('/')
    {
        let start = href.len() - authority.len() + path;
        href.replace_range(..start, "");
    }
    href
}
