//! The menu a decision is chosen from, and validation against it.
//!
//! Jev and Kev both pick from a fixed menu and code validates the result. The real menu has 28
//! questions (one for the book, 27 for presentation settings). This model keeps the four that
//! the documented failures touch: pace, sound, visuals and the book.

pub const WPM: [u32; 5] = [150, 200, 250, 300, 350];
/// Ordered by loudness, so an index doubles as a loudness rank.
pub const SOUNDS: [&str; 4] = ["silent", "rain", "ambient", "synth"];
pub const VISUALS: [&str; 4] = ["off", "fractal", "stars", "neon"];
/// The project docs are unsure whether the catalogue holds 15 or 31 works; 15 is used here.
pub const BOOKS: u32 = 15;

pub const SILENT: u8 = 0;
pub const RAIN: u8 = 1;
pub const AMBIENT: u8 = 2;
pub const SYNTH: u8 = 3;
pub const VISUAL_OFF: u8 = 0;
pub const FRACTAL: u8 = 1;
pub const NEON: u8 = 3;

/// A plan as raw numbers, so that out-of-menu answers can be represented and caught.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub wpm: u32,
    pub sound: u8,
    pub visual: u8,
    pub book: u8,
}

impl Plan {
    /// Everything wrong with this plan; empty when it is on the menu.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !WPM.contains(&self.wpm) {
            out.push(format!("wpm={} is not on the menu", self.wpm));
        }
        if self.sound as usize >= SOUNDS.len() {
            out.push(format!("sound={} is not on the menu", self.sound));
        }
        if self.visual as usize >= VISUALS.len() {
            out.push(format!("visual={} is not on the menu", self.visual));
        }
        if self.book as u32 >= BOOKS {
            out.push(format!("book={} is not on the menu", self.book));
        }
        out
    }

    pub fn is_valid(&self) -> bool {
        self.problems().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_and_invalid_plans() {
        let ok = Plan {
            wpm: 300,
            sound: SYNTH,
            visual: NEON,
            book: 14,
        };
        assert!(ok.is_valid());
        let bad = Plan {
            wpm: 999,
            sound: 9,
            visual: 7,
            book: 40,
        };
        assert_eq!(bad.problems().len(), 4);
    }
}
