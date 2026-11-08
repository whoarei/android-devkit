//! Recolor the original artwork without changing its outline or alpha. The
//! titlebar SVG uses the identical two-ink matrix and shared palette resource.
use crate::{domain::*, preferences::UserPreferences};
use std::{collections::BTreeMap, sync::OnceLock};
use tauri::Manager;

type Rgb = [u8; 3];

fn rgb(hex: &str) -> Rgb {
    std::array::from_fn(|index| u8::from_str_radix(&hex[1 + index * 2..3 + index * 2], 16).unwrap())
}

fn mix(first: Rgb, second: Rgb, amount: f64) -> Rgb {
    std::array::from_fn(|i| {
        (first[i] as f64 + (second[i] as f64 - first[i] as f64) * amount).round() as u8
    })
}

fn luminance(color: Rgb) -> f64 {
    color
        .iter()
        .zip([0.2126, 0.7152, 0.0722])
        .map(|(&value, weight)| {
            let channel = value as f64 / 255.0;
            let linear = if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            };
            linear * weight
        })
        .sum()
}

fn contrast(first: Rgb, second: Rgb) -> f64 {
    let a = luminance(first);
    let b = luminance(second);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn colors(preferences: &UserPreferences, dark: bool) -> (Rgb, Rgb) {
    static PALETTE: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    let palette = PALETTE.get_or_init(|| {
        serde_json::from_str(include_str!("../resources/theme-colors.json"))
            .expect("embedded theme palette")
    });
    let base = rgb(if preferences.color_theme == "custom" {
        &preferences.custom_color
    } else {
        &palette[&preferences.color_theme]
    });
    let toward = rgb("#0a101c");
    let mut fill = (0..=100)
        .map(|step| mix(base, toward, step as f64 / 100.0))
        .find(|&candidate| contrast(candidate, [255; 3]) >= 4.7)
        .unwrap_or(toward);
    if dark {
        fill = (0..=100)
            .map(|step| mix(fill, [255; 3], step as f64 / 100.0))
            .find(|&candidate| contrast(candidate, rgb("#0d1524")) >= 3.0)
            .unwrap_or(fill);
    }
    (fill, [255; 3])
}

fn artwork() -> &'static image::RgbaImage {
    static ARTWORK: OnceLock<image::RgbaImage> = OnceLock::new();
    ARTWORK.get_or_init(|| {
        image::load_from_memory(include_bytes!("../../public/android-devkit.png"))
            .expect("embedded brand artwork")
            .into_rgba8()
    })
}

pub fn render(preferences: &UserPreferences, dark: bool) -> tauri::image::Image<'static> {
    let source = artwork();
    let (fill, foreground) = colors(preferences, dark);
    let mut pixels = source.as_raw().clone();
    for pixel in pixels.chunks_exact_mut(4) {
        let neutral = pixel[0] as f64 / 255.0;
        let material = (pixel[1] as f64 - pixel[0] as f64) / 128.0;
        for i in 0..3 {
            pixel[i] = (foreground[i] as f64 * neutral + fill[i] as f64 * material)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
    tauri::image::Image::new_owned(pixels, source.width(), source.height())
}

pub fn for_handle(handle: &tauri::AppHandle) -> Result<tauri::image::Image<'static>> {
    let preferences = handle
        .state::<std::sync::Arc<crate::app::App>>()
        .store
        .preferences()?;
    let dark = match preferences.theme.as_str() {
        "dark" => true,
        "light" => false,
        _ => {
            handle
                .get_webview_window("main")
                .and_then(|window| window.theme().ok())
                == Some(tauri::Theme::Dark)
        }
    };
    Ok(render(&preferences, dark))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_colors_match_frontend_and_keep_contrast() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../scripts/fixtures/brand-icon-colors.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let preferences = UserPreferences {
                color_theme: case["palette"].as_str().unwrap().into(),
                custom_color: case["customColor"].as_str().unwrap().into(),
                ..UserPreferences::default()
            };
            let (fill, foreground) = colors(&preferences, case["dark"].as_bool().unwrap());
            assert_eq!(fill, rgb(case["fill"].as_str().unwrap()), "{case}");
            assert_eq!(
                foreground,
                rgb(case["foreground"].as_str().unwrap()),
                "{case}"
            );
            assert!(contrast(fill, foreground) >= 4.5, "{case}");
        }
    }

    #[test]
    fn recoloring_preserves_artwork_alpha_and_details() {
        let source = artwork();
        let preferences = UserPreferences::default();
        let light = render(&preferences, false);
        let dark = render(&preferences, true);
        assert_eq!((light.width(), light.height()), source.dimensions());
        assert_ne!(light.rgba(), dark.rgba());
        for icon in [light, dark] {
            for (original, tinted) in source
                .as_raw()
                .chunks_exact(4)
                .zip(icon.rgba().chunks_exact(4))
            {
                assert_eq!(original[3], tinted[3], "alpha/outline changed");
            }
            let colors: std::collections::HashSet<_> = icon
                .rgba()
                .chunks_exact(4)
                .filter(|pixel| pixel[3] == 255)
                .map(|pixel| [pixel[0], pixel[1], pixel[2]])
                .collect();
            assert!(colors.len() > 100, "artwork relief was flattened");
        }
    }
}
