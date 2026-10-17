//! Asset source: Kore's own assets (exact Lucide 0.424 icons used by the original
//! app, app icons) layered over gpui-kit's bundled assets.

use gpui_kit::{AssetSource, Result, SharedString};
use rust_embed::RustEmbed;
use std::borrow::Cow;

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "lucide/*.svg"]
#[include = "kore/*.svg"]
#[include = "icons/*.png"]
struct KoreAssets;

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(f) = KoreAssets::get(path) {
            return Ok(Some(f.data));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut out: Vec<SharedString> = KoreAssets::iter()
            .filter(|p| p.starts_with(path))
            .map(|p| SharedString::from(p.to_string()))
            .collect();
        out.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(out)
    }
}
