use std::collections::HashMap;

pub type Guard = Vec<(u8, u8)>;

pub fn add_range(mut guard: Guard, mut new_range: (u8, u8)) -> Guard {
    if guard.is_empty() {
        return vec![new_range];
    }
    
    let mut result = Vec::new();
    let (mut new_start, mut new_end) = new_range;
    let mut placed = false;

    for (start, end) in guard.iter() {
        if new_end < start - 1 {
            // No overlap and new_range is completely before the current range
            if !placed {
                result.push((new_start, new_end));
                placed = true;
            }
            result.push((*start, *end));
        } else if new_start > end + 1 {
            // No overlap and new_range is completely after the current range
            result.push((*start, *end));
        } else {
            // Overlapping or exactly adjacent, merge the ranges
            new_start = std::cmp::min(new_start, *start);
            new_end = std::cmp::max(new_end, *end);
        }
    }

    // If new_range is not placed yet, it should be added now.
    // This covers the case when new_range extends beyond all existing ranges
    if !placed {
        result.push((new_start, new_end));
    }

    result
}


pub fn intersection(left: &Guard, right: &Guard) -> Guard {
    let mut result = Vec::new();
    let mut i = 0;
    let mut j = 0;

    while i < left.len() && j < right.len() {
        let (start1, end1) = left[i];
        let (start2, end2) = right[j];

        if end1 < start2 {
            i += 1;
        } else if end2 < start1 {
            j += 1;
        } else {
            result.push((std::cmp::max(start1, start2), std::cmp::min(end1, end2)));
            if end1 < end2 {
                i += 1;
            } else {
                j += 1;
            }
        }
    }

    result
}

pub fn subtract(minuend: &Guard, subtrahend: &Guard) -> Guard {
    let mut result = Vec::new();
    let mut remaining = minuend.clone();
    let mut i = 0;

    for &(start, end) in subtrahend {
        while i < remaining.len() {
            let (cur_start, cur_end) = remaining[i];

            if end < cur_start {
                break;
            } else if start > cur_end {
                result.push((cur_start, cur_end));
                i += 1;
            } else {
                if cur_start < start {
                    result.push((cur_start, start - 1));
                }
                if cur_end > end {
                    remaining[i] = (end + 1, cur_end);
                    break;
                } else {
                    i += 1;
                }
            }
        }
    }

    result.extend(remaining.split_off(i));
    result
}

pub fn union(g1: &Guard, g2: &Guard) -> Guard {
    let mut result = Vec::new();
    let mut i = 0;
    let mut j = 0;

    let mut current_start;
    let mut current_end;

    if !g1.is_empty() && !g2.is_empty() {
        if g1[0].0 < g2[0].0 {
            current_start = g1[0].0;
            current_end = g1[0].1;
            i += 1;
        } else {
            current_start = g2[0].0;
            current_end = g2[0].1;
            j += 1;
        }
        result.push((current_start, current_end));
    } else if !g1.is_empty() {
        return g1.clone();
    } else {
        return g2.clone();
    }

    while i < g1.len() && j < g2.len() {
        let (next_start, next_end) = if g1[i].0 < g2[j].0 {
            i += 1;
            g1[i - 1]
        } else {
            j += 1;
            g2[j - 1]
        };

        let last = result.last_mut().unwrap();
        if next_start <= last.1 + 1 {
            last.1 = std::cmp::max(last.1, next_end);
        } else {
            result.push((next_start, next_end));
        }
    }

    for &range in &g1[i..] {
        let last = result.last_mut().unwrap();
        if range.0 <= last.1 + 1 {
            last.1 = std::cmp::max(last.1, range.1);
        } else {
            result.push(range);
        }
    }

    for &range in &g2[j..] {
        let last = result.last_mut().unwrap();
        if range.0 <= last.1 + 1 {
            last.1 = std::cmp::max(last.1, range.1);
        } else {
            result.push(range);
        }
    }

    result
}

pub trait Monoid {
    fn empty() -> Self;
    fn append(&mut self, other: Self);
}

pub fn mintermize<Out: Monoid + Clone, I: Iterator<Item = (Out, Guard)>>
    (input_map: I) -> HashMap<Guard, Out>
{
    let mut leaves = HashMap::new();
    leaves.insert(vec![(0, 255)], Out::empty()); // Global guard with no outputs

    for (out, guard) in input_map {
        let mut new_leaves = HashMap::new();
        // Intersect each current leaf with the new guard
        for (current_guard, current_out) in &leaves {
            let intersection = intersection(&current_guard, &guard);
            if !intersection.is_empty() {
                let leaf_cfg = new_leaves.entry(intersection).or_insert(current_out.clone());
                leaf_cfg.append(out.clone());
            }
        }

        for (current_guard, current_out) in leaves.drain() {
            let subtraction = subtract(&current_guard, &guard);
            if !subtraction.is_empty() {
                new_leaves.entry(subtraction).or_insert(current_out);
            }
        }

        leaves = new_leaves;
    }

    leaves
}



#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;

    #[test]
    fn test_operators() {
        let left = vec![(0, 0), (3, 10), (20, 30), (40, 50), (80, 90)];
        let right = vec![(5, 15), (25, 45), (60, 70)];
        let right2 = vec![(5, 15), (18, 45), (100, 110)];
        let right3 = vec![(5, 7), (9, 45), (100, 110)];
        let right_out = vec![(34, 36), (100, 110)];
        let right_all = vec![(0, 90)];
        let right_in = vec![(5, 6)];
        assert_eq!(intersection(&left, &right), vec![(5, 10), (25, 30), (40, 45)]);
        assert_eq!(intersection(&left, &right2), vec![(5, 10), (20, 30), (40, 45)]);
        assert_eq!(intersection(&left, &right3), vec![(5, 7), (9, 10), (20, 30), (40, 45)]);
        assert_eq!(intersection(&left, &right_out), vec![]);
        assert_eq!(intersection(&left, &right_all), left);
        assert_eq!(intersection(&left, &right_in), [(5, 6)]);
        assert_eq!(subtract(&left, &right), vec![(0, 0), (3, 4), (20, 24), (46, 50), (80, 90)]);
        assert_eq!(subtract(&left, &right2), vec![(0, 0), (3, 4), (46, 50), (80, 90)]);
        assert_eq!(subtract(&left, &right3), vec![(0, 0), (3, 4), (8, 8), (46, 50), (80, 90)]);
        assert_eq!(subtract(&left, &right_out), left);
        assert_eq!(subtract(&left, &right_all), vec![]);
        assert_eq!(union(&left, &right), vec![(0, 0), (3, 15), (20, 50), (60, 70), (80, 90)]);
        assert_eq!(union(&left, &right2), vec![(0, 0), (3, 15), (18, 50), (80, 90), (100, 110)]);
        assert_eq!(union(&left, &right3), vec![(0, 0), (3, 50), (80, 90), (100, 110)]);
        assert_eq!(
            union(&left, &right_out),
            vec![(0, 0), (3, 10), (20, 30), (34, 36), (40, 50), (80, 90), (100, 110)]
        );
        assert_eq!(union(&left, &right_all), vec![(0, 90)]);

        let left = vec![(0, 0), (3, 5)];
        let right = vec![(1, 1), (6, 7)];
        assert_eq!(union(&left, &right), vec![(0, 1), (3, 7)]);

        let left = vec![(0, 0), (3, 5)];
        let right = vec![(1, 2)];
        assert_eq!(union(&left, &right), vec![(0, 5)]);
    }

    impl Monoid for HashSet<usize> {
        fn empty() -> Self {
            HashSet::new()
        }

        fn append(&mut self, other: Self) {
            self.extend(other);
        }
    }

    #[test]
    fn test_mintermize() {
        let input_map = vec![
            (vec![1].into_iter().collect(), vec![(0, 0), (3, 10), (20, 30), (40, 50), (80, 90)]),
            (vec![2].into_iter().collect(), vec![(5, 15), (25, 45), (60, 70)]),
            (vec![3].into_iter().collect(), vec![(5, 15), (18, 45), (100, 110)]),
            (vec![4, 5].into_iter().collect(), vec![(5, 7), (9, 45), (100, 110)]),
        ];

        let result = mintermize(input_map.into_iter());

        // Resulting guards should be disjoint sets that cover the whole universe.
        let mut union_of_guards = Guard::new();
        for guard in result.keys() {
            union_of_guards = union(guard, &union_of_guards);
            for guard2 in result.keys() {
                if guard != guard2 {
                    assert_eq!(intersection(guard, guard2), vec![]);
                    assert_eq!(&subtract(guard, guard2), guard);
                }
            }
        }
        assert_eq!(union_of_guards, vec![(0, 255)]);

        let mut expected = HashMap::new();
        expected.insert(vec![(1, 2), (51, 59), (71, 79), (91, 99), (111, 255)], vec![]);
        expected.insert(vec![(0, 0), (3, 4), (46, 50), (80, 90)], vec![1]);
        expected.insert(vec![(60, 70)], vec![2]);
        expected.insert(vec![(16, 17)], vec![4, 5]);
        expected.insert(vec![(8, 8)], vec![1, 2, 3]);
        expected.insert(vec![(18, 19), (100, 110)], vec![3, 4, 5]);
        expected.insert(vec![(11, 15), (31, 39)], vec![2, 3, 4, 5]);
        expected.insert(vec![(20, 24)], vec![1, 3, 4, 5]);
        expected.insert(vec![(5, 7), (9, 10), (25, 30), (40, 45)], vec![1, 2, 3, 4, 5]);

        let expected = expected.into_iter()
            .map(|(k, v)| (k, v.into_iter().collect::<HashSet<_>>())).collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_add_range() {
        let mut guard = vec![(65, 68), (98, 99)];
        guard = add_range(guard, (66, 67));
        assert_eq!(guard, vec![(65, 68), (98, 99)]);

        let mut guard = vec![(66, 66), (98, 99)];
        guard = add_range(guard, (67, 67));
        assert_eq!(guard, vec![(66, 67), (98, 99)]);
    }
}
