// SPDX-License-Identifier: MIT
mod archive_cover;
mod archive_rar;
pub(crate) fn extract(path: &std::path::Path, password: Option<&str>) -> Result<(), String> {
    archive_rar::run(path, password, &mut std::io::stdout().lock())
}
pub(crate) fn cover(path: &std::path::Path) -> Result<Vec<u8>, String> {
    archive_rar::cover_image(path)
}
