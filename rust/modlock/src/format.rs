//! format writes the numbers a mode's HUD and chat lines show, the way
//! players read them.
//!
//! Each call is a few string operations, cheap enough to run every frame,
//! though a HUD that redraws only on change calls them only then.

/// clock writes seconds as minutes and seconds, such as 4:07, rounding up so
/// a countdown reads 0:00 only once it has run out. Negative time reads 0:00.
pub fn clock(seconds: f64) -> String {
    let whole = seconds.ceil().max(0.0) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// stopwatch writes seconds with hundredths, such as 1:04.37, rounding down
/// so a run's time never reads faster than it was. Negative time reads
/// 0:00.00.
pub fn stopwatch(seconds: f64) -> String {
    let hundredths = (seconds * 100.0).floor().max(0.0) as u64;
    let (minutes, rest) = (hundredths / 6000, hundredths % 6000);
    format!("{minutes}:{:02}.{:02}", rest / 100, rest % 100)
}

/// percent writes part of whole as a whole percentage from 0% to 100%, such
/// as 42%, or a dash while whole is not positive. A fraction is
/// percent(f, 1.0).
pub fn percent(part: f64, whole: f64) -> String {
    // A whole that is not a number is not positive either.
    if whole.is_nan() || whole <= 0.0 {
        return "-".to_owned();
    }
    let share = (part / whole).clamp(0.0, 1.0);
    format!("{}%", (share * 100.0).round())
}

/// grouped writes a number rounded to a whole with comma thousands, such as
/// 4,210.
pub fn grouped(value: f64) -> String {
    // Half rounds up, as the other languages' libraries round it.
    let whole = (value + 0.5).floor();
    let digits = format!("{}", whole.abs());
    let mut text = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if whole < 0.0 {
        text.push('-');
    }
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// ordinal writes a place, such as 1st, 2nd, 11th or 23rd.
pub fn ordinal(place: u32) -> String {
    let suffix = match (place % 100, place % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{place}{suffix}")
}

/// count writes a number of things, in the plural unless one, such as 1 kill
/// or 3 kills. The plural is thing with an s; count_of takes another.
pub fn count(number: u64, thing: &str) -> String {
    count_of(number, thing, &format!("{thing}s"))
}

/// count_of writes a number of things as count does, with the plural given,
/// such as 1 enemy or 3 enemies.
pub fn count_of(number: u64, thing: &str, plural: &str) -> String {
    let name = if number == 1 { thing } else { plural };
    format!("{} {name}", grouped(number as f64))
}

/// clip shortens text to at most width characters, at a word break where one
/// falls in the cut, and marks the cut with an ellipsis.
pub fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let cut: String = text.chars().take(width.saturating_sub(1)).collect();
    let kept = match cut.rfind(' ') {
        Some(space) if space > 0 => &cut[..space],
        _ => cut.as_str(),
    };
    format!("{}…", kept.trim_end())
}
