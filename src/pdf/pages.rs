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
