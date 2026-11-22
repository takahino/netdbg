//! 画面の写し（上の行からの BGRA）に、操作した箇所を塗る。成績書で「どこを押したか・どこに入れたか」が一目で分かる
//! ように、箇所を半透明の色で塗り、濃い色の枠で囲む（下の文字が透けて読める）。

use super::Spot;

/// 写した画面（画素のまま。塗るために持っておく）。
#[derive(Debug, Clone, Default)]
pub struct Screen {
    /// 上の行からの BGRA。
    pub pixels: Vec<u8>,
    /// 画像の左上の、画面の座標。
    pub left: i32,
    pub top: i32,
    pub width: usize,
    pub height: usize,
    /// 写した窓（前にあるものから）。
    pub windows: Vec<usize>,
}

/// 塗る色（BGR）と濃さ。枠の太さ（画素）。
const FILL: [u8; 3] = [0x00, 0x8C, 0xFF];
const FILL_ALPHA: u32 = 90; // 255 分の
const BORDER: [u8; 3] = [0x00, 0x00, 0xE0];
const BORDER_WIDTH: i32 = 3;

/// `screen` の写しの、`spot` を塗ったもの。箇所が画像の外なら `None`。
pub fn painted(screen: &Screen, spot: Spot) -> Option<Vec<u8>> {
    let (width, height) = (screen.width as i32, screen.height as i32);
    // 枠は箇所の外側に描く（箇所の中の文字を隠さない）。
    let outer = Spot { left: spot.left - screen.left - BORDER_WIDTH, top: spot.top - screen.top - BORDER_WIDTH, right: spot.right - screen.left + BORDER_WIDTH, bottom: spot.bottom - screen.top + BORDER_WIDTH };
    let inner = Spot { left: outer.left + BORDER_WIDTH, top: outer.top + BORDER_WIDTH, right: outer.right - BORDER_WIDTH, bottom: outer.bottom - BORDER_WIDTH };
    let clip = |s: Spot| Spot { left: s.left.clamp(0, width), top: s.top.clamp(0, height), right: s.right.clamp(0, width), bottom: s.bottom.clamp(0, height) };
    let area = clip(outer);
    if area.left >= area.right || area.top >= area.bottom || screen.pixels.len() != screen.width * screen.height * 4 {
        return None;
    }
    let mut pixels = screen.pixels.clone();
    for y in area.top..area.bottom {
        for x in area.left..area.right {
            let at = (y as usize * screen.width + x as usize) * 4;
            let pixel = &mut pixels[at..at + 3];
            let inside = x >= inner.left && x < inner.right && y >= inner.top && y < inner.bottom;
            if inside {
                for (channel, color) in pixel.iter_mut().zip(FILL) {
                    *channel = ((*channel as u32 * (255 - FILL_ALPHA) + color as u32 * FILL_ALPHA) / 255) as u8;
                }
            } else {
                pixel.copy_from_slice(&BORDER);
            }
        }
    }
    Some(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white(width: usize, height: usize) -> Screen {
        Screen { pixels: vec![0xFF; width * height * 4], left: 100, top: 200, width, height, windows: vec![] }
    }

    fn pixel(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 3] {
        let at = (y * width + x) * 4;
        [pixels[at], pixels[at + 1], pixels[at + 2]]
    }

    #[test]
    fn the_spot_is_tinted_and_framed_outside() {
        let screen = white(40, 30);
        // 画面の (110, 210)〜(120, 220) は、画像の (10, 10)〜(20, 20)。
        let pixels = painted(&screen, Spot { left: 110, top: 210, right: 120, bottom: 220 }).unwrap();
        let tinted = pixel(&pixels, 40, 15, 15);
        assert!(tinted[0] < 0xFF && tinted[2] == 0xFF, "中は半透明の色: {tinted:?}");
        assert_eq!(pixel(&pixels, 40, 8, 15), BORDER, "枠は箇所の外");
        assert_eq!(pixel(&pixels, 40, 30, 15), [0xFF; 3], "外は変えない");
        assert_eq!(pixel(&screen.pixels, 40, 15, 15), [0xFF; 3], "もとの写しは変えない");
    }

    #[test]
    fn a_spot_outside_the_picture_is_not_painted() {
        assert!(painted(&white(40, 30), Spot { left: 0, top: 0, right: 10, bottom: 10 }).is_none());
        assert!(painted(&white(40, 30), Spot { left: 135, top: 225, right: 160, bottom: 260 }).is_some(), "端にかかれば、かかった分だけ塗る");
    }
}
