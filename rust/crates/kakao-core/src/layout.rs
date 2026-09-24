use crate::model::Rect;
use crate::rules::LayoutRules;

pub fn contains_ad_token(rules: &LayoutRules, text: &str) -> bool {
    contains_any_ad_token(&rules.aggressive_ad_tokens_lc(), text)
}

/// `contains_ad_token` with the lowercased token list computed by the caller,
/// so a subtree walk does not rebuild it at every node.
pub(crate) fn contains_any_ad_token(tokens_lc: &[String], text: &str) -> bool {
    // Empty text can match neither a word nor a non-empty substring. Most
    // CEF/EVA nodes have no text, so skip the lowercase and word set.
    if text.is_empty() {
        return false;
    }
    let low = text.to_lowercase();
    let mut words: Option<std::collections::HashSet<String>> = None;
    for token in tokens_lc {
        if token.is_empty() {
            continue;
        }
        if token.is_ascii()
            && token.chars().all(|ch| ch.is_ascii_alphanumeric())
            && token.len() <= 2
        {
            if words
                .get_or_insert_with(|| ascii_words(&low))
                .contains(token)
            {
                return true;
            }
            continue;
        }
        if low.contains(token.as_str()) {
            return true;
        }
    }
    false
}

fn ascii_words(text: &str) -> std::collections::HashSet<String> {
    let mut words = std::collections::HashSet::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            words.insert(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.insert(current);
    }
    words
}

pub fn is_chrome_widget_class(rules: &LayoutRules, class_name: &str) -> bool {
    rules
        .chrome_widget_prefixes
        .iter()
        .any(|prefix| class_name.starts_with(prefix))
}

pub fn is_bottom_banner_candidate(
    rules: &LayoutRules,
    child_rect: Rect,
    parent_rect: Rect,
) -> bool {
    let height = child_rect.height();
    let width = child_rect.width();
    let parent_width = parent_rect.width();
    if height < rules.banner_min_height_px || height > rules.banner_max_height_px {
        return false;
    }
    if parent_width <= 0 {
        return false;
    }
    if (f64::from(width) / f64::from(parent_width)) < rules.banner_min_width_ratio {
        return false;
    }
    if (child_rect.bottom - parent_rect.bottom).abs() > rules.banner_bottom_margin_px {
        return false;
    }
    true
}

pub fn should_close_empty_eva_child(
    rules: &LayoutRules,
    class_name: &str,
    window_text: &str,
    parent_text: &str,
    has_custom_scroll: bool,
    has_ad_signal: bool,
) -> bool {
    let matches = class_name == rules.eva_child_class
        && window_text.is_empty()
        && !parent_text.is_empty()
        && !has_custom_scroll;
    if !matches {
        return false;
    }
    if rules.close_empty_eva_child_requires_ad_signal && !has_ad_signal {
        return false;
    }
    true
}

pub fn planned_view_resize(
    rules: &LayoutRules,
    window_text: &str,
    parent_rect: Rect,
    current: Option<Rect>,
) -> Option<(i32, i32, i32, i32)> {
    let width = parent_rect.width() - rules.layout_shadow_padding_px;
    let height = if window_text.starts_with(&rules.main_view_prefix) {
        Some(parent_rect.height() - rules.main_view_padding_px)
    } else if window_text.starts_with(&rules.lock_view_prefix) {
        Some(parent_rect.height())
    } else {
        None
    };
    let height = height?;
    if width < 1 || height < 1 {
        return None;
    }
    if let Some(current) = current {
        if current.width() == width && current.height() == height {
            return None;
        }
    }
    Some((0, 0, width, height))
}

#[cfg(test)]
mod token_tests {
    use super::*;

    #[test]
    fn ad_token_matching_keeps_word_boundaries_for_short_tokens() {
        let rules = LayoutRules::default();
        assert!(!contains_ad_token(&rules, ""));
        assert!(contains_ad_token(&rules, "AdFit NAS Advertisement"));
        assert!(contains_ad_token(&rules, "sponsored Ad here"));
        assert!(
            !contains_ad_token(&rules, "Adobe Reader"),
            "short 'ad' needs a word boundary"
        );
        assert!(contains_ad_token(&rules, "오늘의 광고"));
        assert!(!contains_ad_token(&rules, "친구 목록"));
    }
}
