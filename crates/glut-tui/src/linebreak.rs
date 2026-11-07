//! Where a row may break inside a run without spaces (T48): Chinese and Japanese put no
//! spaces between words, so a break at spaces only would move a whole sentence to the next row. Between wide
//! characters a row may break — but a closing mark never starts a row and an opening one never ends it (kinsoku, as
//! UAX #14 has it for the marks Glut's text holds).

use unicode_width::UnicodeWidthChar;

/// Marks that close or follow: a row never starts with one.
const NO_START: &str = "。、，．・：；！？）」』】〕〉》〗〙〛｝］ー々〻ゝゞヽヾぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ,.;:!?)]}%";

/// Marks that open: a row never ends with one.
const NO_END: &str = "（「『【〔〈《〖〘〚｛［([{";

/// Whether a row may break between `before` and `after`, two characters of one run without spaces.
pub fn between(before: char, after: char) -> bool {
    let wide = |c: char| c.width() == Some(2);
    (wide(before) || wide(after)) && !NO_START.contains(after) && !NO_END.contains(before)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_characters_break_between_them_but_never_before_a_closing_mark() {
        assert!(between('中', '文'));
        assert!(between('t', '是'), "Latin before Chinese");
        assert!(!between('文', '。'));
        assert!(!between('文', ','));
        assert!(!between('「', '中'));
        assert!(
            !between('テ', 'ー'),
            "the long vowel mark stays with its kana"
        );
        assert!(!between('a', 'b'), "a Latin word stays whole");
    }
}
