pub fn matches_flatpak_app<'a>(tokens: &'a [String], app_id: &str) -> Option<&'a [String]> {
    match tokens {
        [flatpak, run, token_app_id, tail @ ..]
            if flatpak == "flatpak" && run == "run" && token_app_id == app_id =>
        {
            Some(tail)
        }
        _ => None,
    }
}
