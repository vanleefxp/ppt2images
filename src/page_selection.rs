//! Page-range selection and resolution against a slide count.

use std::num::NonZeroU64;

use bitvec::prelude::BitVec;

/// A validated set of page ranges.
///
/// Page numbers are user-facing numbers whose first value is `--start-index`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageSelection {
    ranges: Vec<PageRange>,
}

/// One comma-separated page range.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PageRange {
    start: Option<u64>,
    end: Option<u64>,
    step: NonZeroU64,
}

/// Resolved page selection stored as one bit per slide.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PagePlan {
    selected: BitVec,
    start_index: u64,
    selected_count: usize,
}

impl PagePlan {
    /// Number of selected pages.
    pub fn len(&self) -> usize {
        self.selected_count
    }

    /// Whether no pages were selected.
    pub fn is_empty(&self) -> bool {
        self.selected_count == 0
    }

    /// Total number of slides represented by the bitmap.
    pub fn total_slides(&self) -> usize {
        self.selected.len()
    }

    /// Whether a zero-based slide index is selected.
    pub fn contains(&self, slide_index: usize) -> bool {
        self.selected
            .get(slide_index)
            .is_some_and(|selected| *selected)
    }

    /// Returns the page number for a selected slide index.
    pub fn page_number(&self, slide_index: usize) -> Option<u64> {
        if !self.contains(slide_index) {
            return None;
        }
        let index = u64::try_from(slide_index).ok()?;
        self.start_index.checked_add(index)
    }

    /// Iterates selected zero-based slide indices in ascending order.
    pub fn iter_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.selected.iter_ones()
    }
}

impl Default for PageSelection {
    fn default() -> Self {
        Self::all()
    }
}

impl PageSelection {
    /// Selects every page.
    pub fn all() -> Self {
        Self {
            ranges: vec![PageRange {
                start: None,
                end: None,
                step: NonZeroU64::MIN,
            }],
        }
    }

    /// Parses a comma-separated page selection such as `1-3,6,9-`.
    pub fn parse(input: &str) -> Result<Self, String> {
        if input.trim().is_empty() {
            return Err("page selection must not be empty".to_owned());
        }
        let mut ranges = Vec::new();
        for token in input.split(',') {
            let token = token.trim();
            if token.is_empty() {
                return Err("page selection contains an empty range".to_owned());
            }
            ranges.push(PageRange::parse(token)?);
        }
        Ok(Self { ranges })
    }

    /// Resolves the selection for a presentation with `slide_count` slides.
    pub fn resolve(&self, start_index: u64, slide_count: usize) -> Result<PagePlan, String> {
        if slide_count == 0 {
            return Err("the presentation contains no slides".to_owned());
        }
        let count = u64::try_from(slide_count)
            .map_err(|_| "slide count is too large for this platform".to_owned())?;
        let last_page = start_index
            .checked_add(count - 1)
            .ok_or_else(|| "page numbering overflow".to_owned())?;

        let mut selected = BitVec::repeat(false, slide_count);
        let mut selected_count = 0;
        for range in &self.ranges {
            let first = range.start.unwrap_or(start_index);
            let last = range.end.unwrap_or(last_page);
            validate_bound(first, start_index, last_page)?;
            validate_bound(last, start_index, last_page)?;
            if first > last {
                return Err(format!("page range start {first} is after its end {last}"));
            }

            let mut page = first;
            loop {
                let index = usize::try_from(page - start_index)
                    .map_err(|_| "page index is too large for this platform".to_owned())?;
                if !selected[index] {
                    selected.set(index, true);
                    selected_count += 1;
                }
                let Some(next) = page.checked_add(range.step.get()) else {
                    break;
                };
                if next > last {
                    break;
                }
                page = next;
            }
        }

        if selected_count == 0 {
            return Err("page selection resolved to no pages".to_owned());
        }
        Ok(PagePlan {
            selected,
            start_index,
            selected_count,
        })
    }
}

impl PageRange {
    fn parse(token: &str) -> Result<Self, String> {
        let (range_text, step_text) = match token.split_once('@') {
            Some((range, step)) => {
                if step.contains('@') {
                    return Err(format!(
                        "invalid page range '{token}': too many '@' separators"
                    ));
                }
                (range, Some(step))
            }
            None => (token, None),
        };

        let range_text = range_text.trim();
        let has_range = range_text.contains('-');
        if range_text.matches('-').count() > 1 {
            return Err(format!("invalid page range '{token}': use a-b, a-, or -b"));
        }

        let (start, end) = if has_range {
            let (left, right) = range_text
                .split_once('-')
                .ok_or_else(|| format!("invalid page range '{token}'"))?;
            let start = parse_optional_page(left.trim(), token)?;
            let end = parse_optional_page(right.trim(), token)?;
            if start.is_none() && end.is_none() {
                return Err(format!(
                    "invalid page range '{token}': specify a start page, an end page, or both"
                ));
            }
            (start, end)
        } else {
            if step_text.is_some() {
                return Err(format!(
                    "invalid page range '{token}': a step requires a range such as a-b@t"
                ));
            }
            let page = parse_page(range_text, token)?;
            (Some(page), Some(page))
        };

        let step = match step_text {
            Some(step) => {
                let step = step
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| format!("invalid step in page range '{token}'"))?;
                NonZeroU64::new(step)
                    .ok_or_else(|| format!("page range '{token}' has a zero step"))?
            }
            None => NonZeroU64::MIN,
        };

        if let (Some(start), Some(end)) = (start, end)
            && start > end
        {
            return Err(format!("page range '{token}' starts after its end page"));
        }

        Ok(Self { start, end, step })
    }
}

fn parse_optional_page(text: &str, token: &str) -> Result<Option<u64>, String> {
    if text.is_empty() {
        Ok(None)
    } else {
        parse_page(text, token).map(Some)
    }
}

fn parse_page(text: &str, token: &str) -> Result<u64, String> {
    text.parse::<u64>()
        .map_err(|_| format!("invalid page number '{text}' in range '{token}'"))
}

fn validate_bound(page: u64, start_index: u64, last_page: u64) -> Result<(), String> {
    if !(start_index..=last_page).contains(&page) {
        return Err(format!(
            "page {page} is outside the valid range {start_index}..={last_page}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::PageSelection;

    fn resolve(selection: &str, start_index: u64, slide_count: usize) -> Vec<u64> {
        let plan = PageSelection::parse(selection)
            .unwrap()
            .resolve(start_index, slide_count)
            .unwrap();
        plan.iter_indices()
            .map(|index| plan.page_number(index).unwrap())
            .collect()
    }

    #[test]
    fn resolves_mixed_ranges() {
        assert_eq!(resolve("1-3,6,9-", 1, 10), vec![1, 2, 3, 6, 9, 10]);
        assert_eq!(resolve("5-", 1, 10), vec![5, 6, 7, 8, 9, 10]);
        assert_eq!(resolve("-4", 1, 10), vec![1, 2, 3, 4]);
        assert_eq!(resolve("6", 1, 10), vec![6]);
    }

    #[test]
    fn resolves_stepped_ranges() {
        assert_eq!(resolve("1-9@2", 1, 10), vec![1, 3, 5, 7, 9]);
        assert_eq!(resolve("8-@3", 1, 10), vec![8]);
        assert_eq!(resolve("-10@4", 1, 10), vec![1, 5, 9]);
        assert_eq!(resolve("1-@2", 1, 10), vec![1, 3, 5, 7, 9]);
    }

    #[test]
    fn merges_overlapping_ranges_in_page_order() {
        assert_eq!(resolve("1-3,2-4,6", 1, 10), vec![1, 2, 3, 4, 6]);
        assert_eq!(resolve("6,1-3", 1, 10), vec![1, 2, 3, 6]);
    }

    #[test]
    fn honors_start_index_and_relative_open_ranges() {
        let plan = PageSelection::parse("5-7").unwrap().resolve(5, 10).unwrap();
        assert_eq!(
            plan.iter_indices()
                .map(|index| plan.page_number(index).unwrap())
                .collect::<Vec<_>>(),
            vec![5, 6, 7]
        );
        assert_eq!(plan.iter_indices().collect::<Vec<_>>(), vec![0, 1, 2]);

        assert_eq!(resolve("-7", 5, 10), vec![5, 6, 7]);
        assert_eq!(resolve("7-@2", 5, 10), vec![7, 9, 11, 13]);
    }

    #[test]
    fn all_ranges_defaults_to_every_page() {
        let plan = PageSelection::default().resolve(1, 4).unwrap();
        assert_eq!(
            plan.iter_indices()
                .map(|index| plan.page_number(index).unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(plan.iter_indices().collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        assert_eq!(plan.len(), 4);
        assert_eq!(plan.total_slides(), 4);
        assert!(plan.contains(0));
        assert!(!plan.contains(4));
        assert_eq!(plan.page_number(4), None);
    }

    #[test]
    fn stores_one_bit_per_slide() {
        let plan = PageSelection::parse("2").unwrap().resolve(1, 10).unwrap();
        assert_eq!(plan.total_slides(), 10);
        assert_eq!(plan.len(), 1);
        assert!(!plan.contains(0));
        assert!(plan.contains(1));
        assert_eq!(plan.page_number(1), Some(2));
    }

    #[test]
    fn rejects_invalid_syntax() {
        for selection in [
            "", ",", "1,,2", "-", "1-2-3", "1-2@", "1-2@0", "1-2@x", "6@2", "1@2@3", "a-b",
            "1-2-@3", "5-3",
        ] {
            assert!(
                PageSelection::parse(selection).is_err(),
                "selection should be rejected: {selection}"
            );
        }
    }

    #[test]
    fn rejects_out_of_range_and_reversed_ranges() {
        for selection in ["0", "11", "1-11", "-0"] {
            assert!(
                PageSelection::parse(selection)
                    .unwrap()
                    .resolve(1, 10)
                    .is_err(),
                "selection should not resolve: {selection}"
            );
        }
        assert!(PageSelection::parse("1-3").unwrap().resolve(5, 10).is_err());
        assert!(PageSelection::parse("3-").unwrap().resolve(5, 2).is_err());
        assert!(PageSelection::default().resolve(1, 0).is_err());
    }
}
