use anyhow::{Result, ensure};

use super::MAX_PDF_PAGES;
use crate::document::MAX_LAYERS;

pub const MAX_PAGE_SELECTION_CHARS: usize = 2048;

/// One-based pages and inclusive ranges, deduplicated in document order.
/// Bound endpoints before expanding them, including adversarial pasted input.
pub fn parse_page_selection(text: &str, page_count: usize) -> Result<Vec<usize>> {
    ensure!(
        (1..=MAX_PDF_PAGES).contains(&page_count),
        "The PDF page information is invalid."
    );
    ensure!(
        text.chars().count() <= MAX_PAGE_SELECTION_CHARS,
        "The page selection is too long."
    );
    ensure!(!text.trim().is_empty(), "Enter pages such as 1, 3-5, 8.");
    let normalized = text.replace(['–', '—'], "-");
    let mut selected = vec![false; page_count + 1];
    let number = |text: &str| -> Result<usize> {
        let text = text.trim();
        ensure!(
            !text.is_empty() && text.bytes().all(|ch| ch.is_ascii_digit()),
            "Enter pages such as 1, 3-5, 8."
        );
        let page = text
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("Choose pages between 1 and {page_count}."))?;
        ensure!(
            (1..=page_count).contains(&page),
            "Choose pages between 1 and {page_count}."
        );
        Ok(page)
    };
    for part in normalized.split(',') {
        let mut range = part.split('-');
        let first = number(range.next().unwrap_or_default())?;
        let last = match range.next() {
            Some(last) => number(last)?,
            None => first,
        };
        ensure!(range.next().is_none(), "Enter pages such as 1, 3-5, 8.");
        ensure!(
            first <= last,
            "Page ranges must run from a lower page to a higher page."
        );
        selected[first..=last].fill(true);
    }
    Ok((1..=page_count).filter(|&page| selected[page]).collect())
}

pub fn validate_page_numbers(pages: &[usize], page_count: usize) -> Result<()> {
    ensure!(!pages.is_empty(), "Choose at least one PDF page.");
    ensure!(
        pages.len() <= MAX_LAYERS,
        "Choose at most {MAX_LAYERS} pages at a time."
    );
    ensure!(
        (1..=MAX_PDF_PAGES).contains(&page_count)
            && pages.iter().all(|&page| (1..=page_count).contains(&page)),
        "Choose pages between 1 and {page_count}."
    );
    ensure!(
        pages.windows(2).all(|pair| pair[0] < pair[1]),
        "Select each PDF page once, in document order."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_lists_normalize_ranges_unicode_dashes_duplicates_and_document_order() {
        assert_eq!(
            parse_page_selection("7-8, 1, 3–5, 4, 1—2", 10).unwrap(),
            vec![1, 2, 3, 4, 5, 7, 8]
        );
        assert_eq!(parse_page_selection(" 0002 , 2 - 2 ", 3).unwrap(), vec![2]);
        assert_eq!(
            parse_page_selection("10000", MAX_PDF_PAGES).unwrap(),
            vec![10_000]
        );
        assert_eq!(parse_page_selection("1", 1).unwrap(), vec![1]);
    }

    #[test]
    fn page_lists_reject_malformed_unbounded_and_out_of_source_ranges() {
        for text in [
            "",
            " ",
            ",",
            "1,",
            ",1",
            "1,,2",
            "0",
            "-1",
            "+1",
            "1.5",
            "all",
            "1 2",
            "1-",
            "-2",
            "3-1",
            "1--3",
            "1-2-3",
            "11",
            "1-11",
            "9999999999999999999999999999999999999999",
        ] {
            assert!(parse_page_selection(text, 10).is_err(), "{text:?}");
        }
        assert!(parse_page_selection("1", 0).is_err());
        assert!(parse_page_selection("1", usize::MAX).is_err());
        assert!(parse_page_selection(&"1".repeat(MAX_PAGE_SELECTION_CHARS + 1), 10).is_err());
        assert!(parse_page_selection("1-18446744073709551615", 10).is_err());
    }

    #[test]
    fn page_numbers_require_a_bounded_unique_selection_in_document_order() {
        assert!(validate_page_numbers(&[1, 3, 10], 10).is_ok());
        for pages in [&[][..], &[0, 1], &[1, 11], &[1, 1], &[3, 1]] {
            assert!(validate_page_numbers(pages, 10).is_err());
        }
        let all = parse_page_selection("1-10000", MAX_PDF_PAGES).unwrap();
        assert_eq!(all.len(), MAX_PDF_PAGES);
        assert!(validate_page_numbers(&all, MAX_PDF_PAGES).is_err());
        assert!(
            validate_page_numbers(&(1..=MAX_LAYERS).collect::<Vec<_>>(), MAX_PDF_PAGES).is_ok()
        );
    }
}

/// Toggle a thumbnail while preserving every other page choice. Compact ranges
/// keep even a large source's All Pages selection within the text-field budget.
pub fn toggle_page_selection(
    text: &str,
    all_pages: bool,
    page: usize,
    page_count: usize,
) -> Result<String> {
    ensure!(
        (1..=MAX_PDF_PAGES).contains(&page_count) && (1..=page_count).contains(&page),
        "The PDF page is no longer available."
    );
    let mut pages = if all_pages {
        (1..=page_count).collect::<Vec<_>>()
    } else if text.trim().is_empty() {
        Vec::new()
    } else {
        parse_page_selection(text, page_count)?
    };
    match pages.binary_search(&page) {
        Ok(index) => {
            pages.remove(index);
        }
        Err(index) => pages.insert(index, page),
    }
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < pages.len() {
        let first = pages[index];
        let mut last = first;
        index += 1;
        while index < pages.len() && pages[index] == last + 1 {
            last = pages[index];
            index += 1;
        }
        ranges.push(if first == last {
            first.to_string()
        } else {
            format!("{first}-{last}")
        });
    }
    let result = ranges.join(", ");
    ensure!(
        result.chars().count() <= MAX_PAGE_SELECTION_CHARS,
        "The page selection is too long. Use fewer ranges."
    );
    Ok(result)
}

#[cfg(test)]
mod toggle_tests {
    use super::*;

    #[test]
    fn thumbnail_toggles_preserve_other_pages_and_allow_an_empty_choice() {
        assert_eq!(toggle_page_selection("", true, 4, 7).unwrap(), "1-3, 5-7");
        assert_eq!(
            toggle_page_selection("1, 3–5, 8", false, 4, 10).unwrap(),
            "1, 3, 5, 8"
        );
        assert_eq!(
            toggle_page_selection("1, 3, 5, 8", false, 4, 10).unwrap(),
            "1, 3-5, 8"
        );
        assert_eq!(toggle_page_selection("", false, 2, 7).unwrap(), "2");
        assert_eq!(toggle_page_selection("2", false, 2, 7).unwrap(), "");
        let large = toggle_page_selection("", true, 5000, MAX_PDF_PAGES).unwrap();
        assert_eq!(large, "1-4999, 5001-10000");
        let pages = parse_page_selection(&large, MAX_PDF_PAGES).unwrap();
        assert_eq!(pages.len(), 9999);
        assert!(!pages.contains(&5000));
        assert!(validate_page_numbers(&pages, MAX_PDF_PAGES).is_err());
    }

    #[test]
    fn thumbnail_toggles_reject_invalid_edits_targets_and_text_overflow() {
        for text in ["1-", "7-3", "bad"] {
            assert!(toggle_page_selection(text, false, 2, 10).is_err());
        }
        for (page, count) in [(0, 7), (8, 7), (1, 0), (1, usize::MAX)] {
            assert!(toggle_page_selection("", true, page, count).is_err());
        }
        let mut text = String::new();
        for page in (1..MAX_PDF_PAGES).step_by(2) {
            let candidate = if text.is_empty() {
                page.to_string()
            } else {
                format!("{text}, {page}")
            };
            if candidate.len() > MAX_PAGE_SELECTION_CHARS {
                break;
            }
            text = candidate;
        }
        assert!(text.len() <= MAX_PAGE_SELECTION_CHARS);
        assert!(toggle_page_selection(&text, false, 10_000, MAX_PDF_PAGES).is_err());
    }
}
