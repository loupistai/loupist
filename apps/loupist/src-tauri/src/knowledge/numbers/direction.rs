//! Local direction cues from financial prose. Each phrase describes the change
//! itself ("fell by", not "fell to", which introduces the resulting level).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Fall,
    Rise,
}

#[derive(Clone, Copy, PartialEq)]
enum Boundary {
    Word,
    // A trailing "down" governs a figure at the end of a clause; "down payment" does not.
    ClauseEnd,
    // Chinese and Japanese words can touch the preceding/following text,
    // and Korean inflections follow the listed stems.
    Cjk,
    // Japanese short price suffixes are ambiguous inside words like 安定 or 高値.
    PriceSuffix,
}

struct Cues {
    boundary: Boundary,
    direction: Direction,
    before: &'static [&'static str],
    after: &'static [&'static str],
}

use Boundary::{Cjk, ClauseEnd, PriceSuffix, Word};
use Direction::{Fall, Rise};

const CUES: &[Cues] = &[
    // English
    Cues {
        boundary: Word,
        direction: Fall,
        before: &[
            "down",
            "down by",
            "fell",
            "fell by",
            "declined",
            "declined by",
            "decline of",
            "decreased",
            "decreased by",
            "decrease of",
            "dropped",
            "dropped by",
            "drop of",
            "loss of",
            "losses of",
            "fall of",
        ],
        after: &[
            "lower",
            "less",
            "below",
            "decline",
            "declines",
            "declined",
            "decrease",
            "decreases",
            "decreased",
            "drop",
            "drops",
            "dropped",
            "fall",
            "falls",
            "loss",
        ],
    },
    Cues {
        boundary: Word,
        direction: Rise,
        before: &[
            "up",
            "up by",
            "rose",
            "rose by",
            "rise of",
            "gained",
            "gained by",
            "gain of",
            "increased",
            "increased by",
            "increase of",
            "grew by",
        ],
        after: &[
            "higher", "more", "above", "rise", "gain", "increase", "growth",
        ],
    },
    Cues {
        boundary: ClauseEnd,
        direction: Fall,
        before: &[],
        after: &["down"],
    },
    Cues {
        boundary: ClauseEnd,
        direction: Rise,
        before: &[],
        after: &["up"],
    },
    // Korean
    Cues {
        boundary: Cjk,
        direction: Fall,
        before: &["감소", "하락", "손실"],
        after: &[
            "감소", "줄었", "줄어", "줄고", "줄인", "줄여", "하락", "떨어", "낮아", "낮은", "축소",
            "급감", "급락", "손실",
        ],
    },
    Cues {
        boundary: Cjk,
        direction: Rise,
        before: &["상승", "증가"],
        after: &["상승", "증가", "늘었", "급등", "올랐"],
    },
    // Chinese (traditional and simplified), including a narrowing decline.
    Cues {
        boundary: Cjk,
        direction: Fall,
        before: &[
            "跌幅收窄至",
            "下跌",
            "跌幅",
            "跌",
            "下降",
            "减少",
            "減少",
            "亏损",
            "虧損",
        ],
        after: &["下跌", "下降", "减少", "減少", "亏损", "虧損"],
    },
    Cues {
        boundary: Cjk,
        direction: Rise,
        before: &["上涨", "上漲", "涨幅", "漲幅", "增长", "增長"],
        after: &["上涨", "上漲", "增长", "增長"],
    },
    // Japanese (減少 is shared with Chinese).
    Cues {
        boundary: Cjk,
        direction: Fall,
        before: &["下落", "赤字"],
        after: &["下落", "赤字"],
    },
    Cues {
        boundary: Cjk,
        direction: Rise,
        before: &["上昇", "増加"],
        after: &["上昇", "増加"],
    },
    Cues {
        boundary: PriceSuffix,
        direction: Fall,
        before: &[],
        after: &["安"],
    },
    Cues {
        boundary: PriceSuffix,
        direction: Rise,
        before: &[],
        after: &["高"],
    },
    // German
    Cues {
        boundary: Word,
        direction: Fall,
        before: &[
            "sank um",
            "fiel um",
            "rückgang um",
            "rückgang von",
            "verlust von",
            "minus",
        ],
        after: &["niedriger", "weniger", "rückgang", "verlust"],
    },
    Cues {
        boundary: Word,
        direction: Rise,
        before: &[
            "stieg um",
            "anstieg um",
            "anstieg von",
            "zuwachs von",
            "plus",
        ],
        after: &["höher", "mehr", "zuwachs"],
    },
    // French
    Cues {
        boundary: Word,
        direction: Fall,
        before: &["baisse de", "recul de", "perte de", "baissé de", "chuté de"],
        after: &["de baisse", "de recul", "de perte"],
    },
    Cues {
        boundary: Word,
        direction: Rise,
        before: &["hausse de", "gain de", "progression de", "augmenté de"],
        after: &["de hausse", "de gain"],
    },
    // Spanish
    Cues {
        boundary: Word,
        direction: Fall,
        before: &[
            "cayó",
            "cayó un",
            "bajó",
            "bajó un",
            "caída del",
            "caída de",
            "descenso de",
            "pérdida de",
        ],
        after: &["de caída", "de descenso"],
    },
    Cues {
        boundary: Word,
        direction: Rise,
        before: &[
            "subió",
            "subió un",
            "creció un",
            "alza del",
            "alza de",
            "aumento de",
            "ganancia de",
        ],
        after: &["de alza", "de aumento"],
    },
];

fn gap(c: char) -> bool {
    matches!(c, ' ' | '\u{a0}' | '\u{202f}' | '\u{3000}')
}

// Bound both lookups even for a document without spaces. The longest cue is
// shorter than the window, leaving room to check the adjoining word boundary.
pub(super) fn around(before: &str, after: &str) -> Option<Direction> {
    let before = &before[before
        .char_indices()
        .rev()
        .nth(24)
        .map_or(0, |(i, c)| i + c.len_utf8())..];
    let left = before.trim_end_matches(gap);
    if before[left.len()..].chars().count() <= 2 && left.ends_with(char::is_alphabetic) {
        let left = left.to_lowercase();
        for cue in CUES {
            if cue.before.iter().any(|word| {
                left.strip_suffix(word).is_some_and(|head| {
                    cue.boundary != Word || !head.ends_with(char::is_alphanumeric)
                })
            }) {
                return Some(cue.direction);
            }
        }
    }
    let after = &after[..after.char_indices().nth(24).map_or(after.len(), |(i, _)| i)];
    let right = after.trim_start_matches(gap);
    if after[..after.len() - right.len()].chars().count() > 2
        || !right.starts_with(char::is_alphabetic)
    {
        return None;
    }
    let right = right.to_lowercase();
    // In "100% declined by 4%" the verb and preposition introduce the
    // second number. The first is just the level from which it changed.
    if let Some(digit) = right.find(char::is_numeric) {
        let lead = &right[..digit];
        let lead = super::currency::before(lead)
            .map_or(lead, |(_, start)| &lead[..start])
            .trim_end_matches(gap);
        if CUES.iter().any(|cue| cue.before.contains(&lead)) {
            return None;
        }
    }
    for cue in CUES {
        if cue.after.iter().any(|word| {
            right.strip_prefix(word).is_some_and(|tail| {
                // A cue introducing the next number belongs to that number.
                !tail.trim_start_matches(gap).starts_with(char::is_numeric)
                    && ![" to ", " from ", "至", "到"]
                        .iter()
                        .any(|link| tail.starts_with(link))
                    && match cue.boundary {
                        Word => !tail.starts_with(char::is_alphanumeric),
                        Cjk => true,
                        ClauseEnd => !tail
                            .trim_start_matches(gap)
                            .starts_with(char::is_alphanumeric),
                        PriceSuffix => {
                            !tail.starts_with(char::is_alphabetic)
                                || tail.starts_with(['と', 'で', 'に', 'を', 'の'])
                        }
                    }
            })
        }) {
            return Some(cue.direction);
        }
    }
    None
}
