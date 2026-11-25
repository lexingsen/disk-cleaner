use gpui::{rgb, Rgba};

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub bg: u32,
    pub sidebar: u32,
    pub card: u32,
    pub card_alt: u32,
    pub border: u32,
    pub text: u32,
    pub secondary: u32,
    pub tertiary: u32,
    pub accent: u32,
    pub track: u32,
    pub hover: u32,
    pub field: u32,
}

impl Theme {
    pub fn light() -> Self {
        Theme {
            dark: false,
            bg: 0xf7f7f9,
            sidebar: 0xededf1,
            card: 0xffffff,
            card_alt: 0xf1f1f5,
            border: 0xdedee3,
            text: 0x1c1c1e,
            secondary: 0x6b6b70,
            tertiary: 0xa0a0a6,
            accent: 0x0a84ff,
            track: 0xe4e4e9,
            hover: 0xe8e8ee,
            field: 0xffffff,
        }
    }

    pub fn dark() -> Self {
        Theme {
            dark: true,
            bg: 0x1c1c1e,
            sidebar: 0x232325,
            card: 0x2a2a2c,
            card_alt: 0x323234,
            border: 0x3a3a3d,
            text: 0xf2f2f4,
            secondary: 0xa8a8ae,
            tertiary: 0x74747a,
            accent: 0x0a84ff,
            track: 0x3a3a3d,
            hover: 0x353538,
            field: 0x252527,
        }
    }

    pub fn bg(&self) -> Rgba {
        rgb(self.bg)
    }
    pub fn sidebar(&self) -> Rgba {
        rgb(self.sidebar)
    }
    pub fn card(&self) -> Rgba {
        rgb(self.card)
    }
    pub fn card_alt(&self) -> Rgba {
        rgb(self.card_alt)
    }
    pub fn border(&self) -> Rgba {
        rgb(self.border)
    }
    pub fn text(&self) -> Rgba {
        rgb(self.text)
    }
    pub fn secondary(&self) -> Rgba {
        rgb(self.secondary)
    }
    pub fn tertiary(&self) -> Rgba {
        rgb(self.tertiary)
    }
    pub fn accent(&self) -> Rgba {
        rgb(self.accent)
    }
    pub fn track(&self) -> Rgba {
        rgb(self.track)
    }
    pub fn hover(&self) -> Rgba {
        rgb(self.hover)
    }
    pub fn field(&self) -> Rgba {
        rgb(self.field)
    }
    pub fn success(&self) -> Rgba {
        rgb(0x34c759)
    }
    pub fn warning(&self) -> Rgba {
        rgb(0xff9f0a)
    }
    pub fn danger(&self) -> Rgba {
        rgb(0xff453a)
    }
}

pub fn alpha(hex: u32, a: u8) -> Rgba {
    gpui::rgba((hex << 8) | a as u32)
}

pub fn hue_color(seed: &str) -> Rgba {
    let mut h: u32 = 0;
    for c in seed.chars() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    let hue = (h % 360) as f32;
    hsv(hue, 0.55, 0.85)
}

fn hsv(h: f32, s: f32, v: f32) -> Rgba {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as i32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    gpui::rgba(
        (((r + m) * 255.0) as u32) << 24
            | (((g + m) * 255.0) as u32) << 16
            | (((b + m) * 255.0) as u32) << 8
            | 0xff,
    )
}

pub fn kind_color(hex: u32, a: u8) -> Rgba {
    alpha(hex, a)
}
