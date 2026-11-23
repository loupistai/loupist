//! Local direction cues from financial prose. Each phrase describes the change
//! itself ("fell by", not "fell to", which introduces the resulting level).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Fall,
    Rise,
    /// A negative value in a row or field that is an outflow by nature.
    Outflow,
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
            "outflow of",
            "outflows of",
            "lower by",
            "falls",
            "falling",
            "drops",
            "dropping",
            "declines",
            "declining",
            "lost",
            "loses",
            "losing",
            "shed",
            "sheds",
            "slid",
            "slides",
            "slipped",
            "slips",
            "sank",
            "sinks",
            "tumbled",
            "tumbles",
            "plunged",
            "plunges",
            "plummeted",
            "slumped",
            "retreated",
            "eased",
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
            "outflow",
            "outflows",
            "slide",
            "slump",
            "plunge",
            "tumble",
            "retreat",
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
            "higher by",
            "grew",
            "rises",
            "rising",
            "gains",
            "gaining",
            "increases",
            "climbed",
            "climbs",
            "jumped",
            "jumps",
            "surged",
            "surges",
            "soared",
            "advanced",
            "advances",
            "added",
            "adds",
            "rallied",
            "rebounded",
        ],
        after: &[
            "higher",
            "more",
            "above",
            "rise",
            "rises",
            "gain",
            "gains",
            "increase",
            "increases",
            "increased",
            "growth",
            "jump",
            "surge",
            "rally",
            "climb",
            "advance",
            "rebound",
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
        before: &[
            "감소",
            "하락",
            "손실",
            "하락률",
            "하락폭",
            "낙폭",
            "감소율",
            "감소폭",
        ],
        after: &[
            "감소",
            "줄었",
            "줄어",
            "줄고",
            "줄인",
            "줄여",
            "줄며",
            "하락",
            "떨어",
            "낮아",
            "낮은",
            "낮춰",
            "낮췄",
            "낮춘",
            "축소",
            "급감",
            "급락",
            "폭락",
            "약세",
            "후퇴",
            "하향",
            "손실",
            "유출",
            "내린",
            "내려",
            "내렸",
            "내리",
            "내림",
            "빠진",
            "빠졌",
            "빠져",
            "빠지",
            "밀린",
            "밀려",
            "밀렸",
            "밀리",
            "뒷걸음",
        ],
    },
    Cues {
        boundary: Cjk,
        direction: Rise,
        before: &[
            "상승",
            "증가",
            "상승률",
            "상승폭",
            "오름폭",
            "증가율",
            "증가폭",
        ],
        after: &[
            "상승", "증가", "늘었", "늘어", "늘고", "늘며", "늘린", "늘려", "급등", "폭등", "급증",
            "올랐", "오른", "올라", "오르", "올린", "올려", "올렸", "뛰었", "뛴", "뛰어", "뛰며",
            "치솟", "높아", "높은", "높여", "강세", "반등", "상향", "확대",
        ],
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
            "降幅",
            "下降",
            "降",
            "降低",
            "减少",
            "減少",
            "下滑",
            "回落",
            "走低",
            "挫",
            "亏损",
            "虧損",
        ],
        after: &[
            "下跌", "下降", "减少", "減少", "下滑", "回落", "走低", "亏损", "虧損", "流出",
        ],
    },
    Cues {
        boundary: Cjk,
        direction: Rise,
        before: &[
            "上涨", "上漲", "涨", "漲", "涨幅", "漲幅", "升", "升幅", "增长", "增長", "增加",
            "攀升", "回升", "走高", "反弹", "反彈",
        ],
        after: &[
            "上涨", "上漲", "上升", "增长", "增長", "攀升", "回升", "走高", "反弹", "反彈",
        ],
    },
    // Japanese (減少 is shared with Chinese).
    Cues {
        boundary: Cjk,
        direction: Fall,
        before: &["下落", "下落率", "値下がり", "低下", "赤字"],
        after: &[
            "下落",
            "値下がり",
            "下げ",
            "低下",
            "減",
            "反落",
            "急落",
            "続落",
            "赤字",
        ],
    },
    Cues {
        boundary: Cjk,
        direction: Rise,
        before: &["上昇", "上昇率", "値上がり", "増加"],
        after: &[
            "上昇",
            "値上がり",
            "上げ",
            "増",
            "反発",
            "急騰",
            "続伸",
            "高騰",
        ],
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

/// Which side of its figure a direction word stands on.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Before,
    After,
}

/// Units that name an index move in points (`200.14포인트`, `200.14 points`,
/// `200.14ポイント`, `200.14点`): a direction word after the unit still
/// touches the figure.
const POINT_WORDS: &[&str] = &[
    "points",
    "point",
    "pts",
    "pt",
    "포인트",
    "ポイント",
    "点",
    "點",
];

fn hangul(c: char) -> bool {
    matches!(c, '가'..='힣')
}

fn introduces_number(word: &str) -> bool {
    CUES.iter().any(|cue| cue.before.contains(&word))
}

/// `text` without a leading point unit and up to two spaces after it.
fn after_points(text: &str) -> &str {
    for word in POINT_WORDS {
        let Some(tail) = text
            .get(..word.len())
            .filter(|head| head.eq_ignore_ascii_case(word))
            .map(|_| &text[word.len()..])
        else {
            continue;
        };
        if word.is_ascii() && tail.starts_with(char::is_alphanumeric) {
            continue;
        }
        let rest = tail.trim_start_matches(gap);
        if tail[..tail.len() - rest.len()].chars().count() <= 2 {
            return rest;
        }
    }
    text
}

pub(super) fn around(before: &str, after: &str) -> Option<Direction> {
    read(before, after).map(|(direction, _)| direction)
}

// Bound both lookups even for a document without spaces. The longest cue is
// shorter than the window, leaving room to check the adjoining word boundary.
fn read(before: &str, after: &str) -> Option<(Direction, Side)> {
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
                return Some((cue.direction, Side::Before));
            }
        }
    }
    following(after).map(|direction| (direction, Side::After))
}

/// The direction a word right after a figure (or after its point unit)
/// gives it.
fn following(after: &str) -> Option<Direction> {
    let after = &after[..after.char_indices().nth(32).map_or(after.len(), |(i, _)| i)];
    let right = after.trim_start_matches(gap);
    if after[..after.len() - right.len()].chars().count() > 2 {
        return None;
    }
    let right = after_points(right);
    if !right.starts_with(char::is_alphabetic) {
        return None;
    }
    let right = right.to_lowercase();
    // In "100% declined by 4%" the verb and preposition introduce the
    // second number. The first is just the level from which it changed.
    // A Korean word follows the change it states (`3.31% 하락 4,708.54`).
    if let Some(digit) = right.find(char::is_numeric) {
        let lead = &right[..digit];
        let lead = super::currency::before(lead)
            .map_or(lead, |(_, start)| &lead[..start])
            .trim_end_matches(gap);
        if introduces_number(lead) && !lead.starts_with(hangul) {
            return None;
        }
    }
    for cue in CUES {
        if cue.after.iter().any(|word| {
            right.strip_prefix(word).is_some_and(|tail| {
                // A word that introduces a number (`下跌2%`) belongs to the
                // number after it, but not a Korean one (`1.58% 내린 635.45`).
                !(tail.trim_start_matches(gap).starts_with(char::is_numeric)
                    && introduces_number(word)
                    && !word.starts_with(hangul))
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

/// One figure as the scanner read it.
pub(super) struct Figure {
    /// Where wording before the figure ends (before a currency prefix).
    pub lead: usize,
    pub end: usize,
    /// A printed sign or a non-positive value: no word changes it.
    pub fixed: bool,
    /// A percent, points or basis points: a change rather than a level.
    pub rate: bool,
}

/// The direction the wording gives each figure
/// (docs/analysis/number-checks.md#direction-words). A word touching a figure
/// governs it. A figure in brackets right after another figure restates the
/// same change (`200.14포인트(4.11%) 내린`, `down 200.14 points (4.11%)`,
/// `200.14 points, or 4.11%, lower`): the word of either governs both. A word
/// after the last rate of a list governs every rate before it
/// (`삼성전자 3.06%·SK하이닉스 2.52% 하락`). Nothing reaches across a clause or
/// sentence end, or past a figure with a word of its own.
pub(super) fn governing(text: &str, figures: &[Figure]) -> Vec<Option<Direction>> {
    let mut read: Vec<Option<(Direction, Side)>> = figures
        .iter()
        .map(|figure| {
            (!figure.fixed)
                .then(|| text.get(..figure.lead).zip(text.get(figure.end..)))
                .flatten()
                .and_then(|(before, after)| self::read(before, after))
        })
        .collect();
    for i in 1..figures.len() {
        let (first, second) = (&figures[i - 1], &figures[i]);
        if first.fixed || second.fixed {
            continue;
        }
        let Some(close) = restatement(text, first, second) else {
            continue;
        };
        let shared = read[i - 1].or(read[i]).or_else(|| {
            text.get(close..)
                .and_then(following)
                .map(|direction| (direction, Side::After))
        });
        read[i - 1] = read[i - 1].or(shared);
        read[i] = read[i].or(shared);
    }
    // One word between two figures states one change: a Korean word keeps
    // the figure before it, and the level after it takes nothing.
    for i in 1..figures.len() {
        if let (Some((_, Side::After)), Some((_, Side::Before))) = (read[i - 1], read[i]) {
            if text
                .get(figures[i - 1].end..figures[i].lead)
                .map(|between| between.trim_start_matches([')', '）']).trim_matches(gap))
                .is_some_and(|word| !word.is_empty() && word.chars().all(hangul))
            {
                read[i] = None;
            }
        }
    }
    for i in (1..figures.len()).rev() {
        let (item, last) = (&figures[i - 1], &figures[i]);
        if read[i - 1].is_some() || item.fixed || !item.rate || !last.rate {
            continue;
        }
        let Some((direction, Side::After)) = read[i] else {
            continue;
        };
        if text.get(item.end..last.lead).is_some_and(listed)
            && !text.get(..item.lead).is_some_and(introduced)
        {
            read[i - 1] = Some((direction, Side::After));
        }
    }
    read.into_iter()
        .map(|read| read.map(|(direction, _)| direction))
        .collect()
}

/// Where a governing word may follow when `second` restates `first`: after
/// the bracket that holds it, or after the comma that ends `, or 4.11%,`.
fn restatement(text: &str, first: &Figure, second: &Figure) -> Option<usize> {
    let between = text.get(first.end..second.lead)?;
    if between.len() > 48 {
        return None;
    }
    let head = after_points(between.trim_start_matches(gap));
    let rest = text.get(second.end..)?;
    if let Some(head) = head
        .trim_end_matches(gap)
        .strip_suffix(['(', '（'])
        .map(|head| head.trim_end_matches(gap))
    {
        // Only the unit or the first figure's own word stands before the
        // bracket: `점유율(25%) 하락` is a level, not a restatement.
        let worded = head.is_empty()
            || (!head.contains(gap)
                && head.chars().count() <= 6
                && head.chars().all(char::is_alphabetic)
                && following(head).is_some());
        // `4.11%(200.14포인트)`: the second figure's unit stays inside.
        let inside = after_points(rest);
        let close = inside.chars().next().filter(|c| matches!(c, ')' | '）'))?;
        return worded.then_some(second.end + rest.len() - inside.len() + close.len_utf8());
    }
    let or = head
        .strip_prefix(',')
        .unwrap_or(head)
        .trim_start_matches(gap);
    let or = or.strip_prefix("or")?;
    if !(or.starts_with(gap) && or.trim_start_matches(gap).is_empty()) {
        return None;
    }
    Some(second.end + rest.strip_prefix(',').map_or(0, |_| 1))
}

/// Text between two rates of one list: a separator, then at most a short
/// name (`·SK하이닉스 `, ` and SK Hynix `, `、`, `, `).
fn listed(between: &str) -> bool {
    let text = between.trim_start_matches(gap);
    // Particles such as 와 or と also open a comparison (`3.06%와 비교해`).
    let Some(name) = [
        "·", "・", "･", "‧", "、", ",", "，", "/", "&", "和", "及", "与", "與",
    ]
    .iter()
    .find_map(|separator| text.strip_prefix(separator))
    .or_else(|| {
        ["and", "및"].iter().find_map(|word| {
            text.strip_prefix(word)
                .filter(|rest| rest.is_empty() || rest.starts_with(gap))
        })
    }) else {
        return false;
    };
    let name = name.trim_matches(gap);
    let words: Vec<_> = name.split(gap).filter(|word| !word.is_empty()).collect();
    words.len() <= 3
        && name.chars().count() <= 24
        && name
            .chars()
            .all(|c| c.is_alphabetic() || gap(c) || matches!(c, '-' | '&' | '\''))
        // A Latin name is capitalized (`SK Hynix`), not prose (`with stocks`).
        && words.iter().all(|word| {
            !word.starts_with(|c: char| c.is_ascii_alphabetic())
                || word.starts_with(|c: char| c.is_ascii_uppercase())
        })
}

/// A rate introduced by a word, as a level is (`fell to 3.5%`, `내린 3.5%`).
fn introduced(before: &str) -> bool {
    let before = before.trim_end_matches(gap);
    let before = &before[before
        .char_indices()
        .rev()
        .nth(16)
        .map_or(0, |(i, c)| i + c.len_utf8())..];
    let before = before.to_lowercase();
    CUES.iter()
        .flat_map(|cue| cue.after.iter().chain(cue.before))
        .chain(&["to", "at", "至", "到"])
        .any(|word| {
            before
                .strip_suffix(word)
                .is_some_and(|head| !word.is_ascii() || !head.ends_with(char::is_alphanumeric))
        })
}

/// What a statement row, a data field or an answer's clause names, by its
/// nature (docs/analysis/number-checks.md#outflows-stated-as-amounts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineNature {
    /// Spending or a deduction. Sources print it negative or in brackets;
    /// prose states the amount.
    Outflow,
    /// A loss named as such: its amount is a negative result.
    Loss,
    /// Either sign, and the sign is the meaning: income or loss, a change, a
    /// growth rate, a margin, free cash flow, cash flow from operations.
    Signed,
    /// Another line, whose amount is never spending (revenue, assets).
    Other,
}

use LineNature::{Loss, Other, Outflow, Signed};

/// Row-nature phrases, tried in this order: words that let a row take either
/// sign decide first, then spending, a named loss, a line whose sign carries
/// its meaning, and another line. Latin phrases match whole words; CJK
/// phrases match inside words.
const LINE_NATURES: &[(LineNature, &[&str])] = &[
    (
        Signed,
        &[
            "change",
            "changes",
            "changed",
            "increase",
            "increases",
            "increased",
            "decrease",
            "decreases",
            "decreased",
            "growth",
            "margin",
            "margins",
            "provided by",
            "(used in)",
            "(loss)",
            "(gain)",
            "gain",
            "gains",
            "(expense)",
            "(income)",
            "(benefit)",
            "benefit from",
            "surplus",
            "yoy",
            "y/y",
            "qoq",
            "q/q",
            "variance",
            "variation",
            "difference",
            "delta",
            "증감",
            "증가",
            "감소",
            "변동",
            "변화",
            "성장",
            "이익률",
            "마진",
            "손익",
            "차이",
            "增减",
            "增減",
            "变动",
            "變動",
            "变化",
            "變化",
            "增加",
            "减少",
            "減少",
            "增长",
            "增長",
            "利润率",
            "利潤率",
            "毛利率",
            "损益",
            "損益",
            "同比",
            "环比",
            "環比",
            "増減",
            "増加",
            "成長",
            "利益率",
            "前年比",
            "前期比",
        ],
    ),
    (
        Outflow,
        &[
            "capital expenditure",
            "capital expenditures",
            "capex",
            "capital spending",
            "capital investment",
            "capital investments",
            "expenditure",
            "expenditures",
            "spending",
            "spent",
            "outflow",
            "outflows",
            "purchase of",
            "purchases of",
            "payment of",
            "payments of",
            "payment for",
            "payments for",
            "payment to",
            "payments to",
            "repayment",
            "repayments",
            "repurchase",
            "repurchases",
            "repurchased",
            "buyback",
            "buybacks",
            "dividends paid",
            "dividend paid",
            "taxes paid",
            "tax paid",
            "interest paid",
            "cash paid",
            "cash used in",
            "cash used for",
            "investment in property",
            "investments in property",
            "additions to",
            "acquisition of",
            "acquisitions of",
            "acquisitions",
            "expense",
            "expenses",
            "cost of",
            "costs of",
            "costs",
            "research and development",
            "selling, general and administrative",
            "r&d",
            "sg&a",
            "설비투자",
            "설비 투자",
            "자본적 지출",
            "자본적지출",
            "자본지출",
            "자본 지출",
            "현금 유출",
            "현금유출",
            "유출",
            "지출",
            "취득",
            "유형자산 투자",
            "유형자산투자",
            "자사주 매입",
            "자사주매입",
            "배당금의 지급",
            "배당금 지급",
            "배당금지급",
            "차입금의 상환",
            "차입금 상환",
            "사채의 상환",
            "사채 상환",
            "상환",
            "이자의 지급",
            "이자 지급",
            "법인세의 납부",
            "법인세 납부",
            "매출원가",
            "판매비와관리비",
            "판매비와 관리비",
            "판관비",
            "영업비용",
            "연구개발비",
            "비용",
            "资本支出",
            "資本支出",
            "资本开支",
            "資本開支",
            "购建固定资产",
            "購建固定資產",
            "支付的现金",
            "支付的現金",
            "偿还债务",
            "償還債務",
            "分配股利",
            "支付股利",
            "回购",
            "回購",
            "现金流出",
            "現金流出",
            "营业成本",
            "營業成本",
            "销售费用",
            "銷售費用",
            "管理费用",
            "管理費用",
            "研发费用",
            "研發費用",
            "利息支出",
            "支出",
            "支付",
            "购买",
            "購買",
            "設備投資",
            "資本的支出",
            "取得",
            "購入",
            "支払",
            "借入金の返済",
            "社債の償還",
            "売上原価",
            "販売費及び一般管理費",
            "研究開発費",
        ],
    ),
    (
        Loss,
        &[
            "net loss",
            "net losses",
            "operating loss",
            "operating losses",
            "loss",
            "losses",
            "deficit",
            "순손실",
            "영업손실",
            "손실",
            "적자",
            "净亏损",
            "淨虧損",
            "亏损",
            "虧損",
            "赤字",
            "純損失",
            "営業損失",
            "損失",
        ],
    ),
    (
        Signed,
        &[
            "net income",
            "net earnings",
            "net profit",
            "operating income",
            "operating profit",
            "income from operations",
            "ebit",
            "ebitda",
            "free cash flow",
            "fcf",
            "operating cash flow",
            "cash flow",
            "cash flows",
            "cash from operations",
            "eps",
            "earnings per share",
            "net debt",
            "net cash",
            "net interest",
            "income",
            "profit",
            "earnings",
            "return",
            "returns",
            "순이익",
            "영업이익",
            "세전이익",
            "이익",
            "잉여현금흐름",
            "영업현금흐름",
            "현금흐름",
            "수익률",
            "순현금",
            "순차입금",
            "净利润",
            "淨利潤",
            "营业利润",
            "營業利潤",
            "利润",
            "利潤",
            "自由现金流",
            "自由現金流",
            "现金流",
            "現金流",
            "每股收益",
            "每股盈餘",
            "收益率",
            "純利益",
            "営業利益",
            "経常利益",
            "利益",
            "キャッシュフロー",
            "キャッシュ・フロー",
        ],
    ),
    (
        Other,
        &[
            "revenue",
            "revenues",
            "sales",
            "turnover",
            "assets",
            "liabilities",
            "equity",
            "cash and cash equivalents",
            "cash and investments",
            "cash, cash equivalents",
            "deliveries",
            "production",
            "backlog",
            "debt",
            "price",
            "shares",
            "market cap",
            "market capitalization",
            "dividend",
            "dividends",
            "book value",
            "매출",
            "매출액",
            "자산",
            "부채",
            "자본",
            "현금성자산",
            "인도량",
            "생산량",
            "주가",
            "시가총액",
            "배당",
            "营收",
            "營收",
            "营业收入",
            "營業收入",
            "收入",
            "销售额",
            "銷售額",
            "资产",
            "資產",
            "负债",
            "負債",
            "股价",
            "股價",
            "市值",
            "売上",
            "売上高",
            "収益",
            "資産",
            "負債",
            "株価",
            "時価総額",
        ],
    ),
];

/// Words that state an outflow of the amount they name.
const OUTFLOW_WORDS: &[&str] = &["outflow", "outflows", "cash used", "spent", "유출", "流出"];

/// Lowercase with runs of whitespace and `_` as one space, so a PDF row
/// (`Capital\texpenditures`) and a field (`capital_expenditure`) read as words.
fn words(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_whitespace() || c == '_' {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out.trim_end().to_owned()
}

fn names(text: &str, phrase: &str) -> bool {
    let latin = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    let edge = phrase
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    let tail = phrase
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    text.match_indices(phrase).any(|(at, _)| {
        !(edge && latin(text[..at].chars().next_back()))
            && !(tail && latin(text[at + phrase.len()..].chars().next()))
    })
}

/// The nature of the line a label, field name or clause names, if any.
pub(crate) fn line_nature(label: &str) -> Option<LineNature> {
    let text = words(label);
    LINE_NATURES
        .iter()
        .find(|(_, phrases)| phrases.iter().any(|phrase| names(&text, phrase)))
        .map(|(nature, _)| *nature)
}

/// A clause that says its amount flowed out (`cash outflow of`, `현금 유출`).
pub(crate) fn says_outflow(clause: &str) -> bool {
    let text = words(clause);
    OUTFLOW_WORDS.iter().any(|word| names(&text, word))
}
