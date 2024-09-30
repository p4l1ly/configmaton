
pub struct Intersection {
    pub inputs: Vec<usize>,
    pub neg_watch_mask: Vec<bool>,

    // Everything left of the watch is considered satisfied, but we recheck (and potentially
    // reorder) all the inputs before claiming that the intersection is satisfied.
    pub watch: usize,
}

impl Intersection {
    pub fn is_satisfied(&self) -> bool {
        self.watch == self.inputs.len()
    }
}

pub struct Solver {
    pub intersections: Vec<Intersection>,
    pub positive_watches: Vec<Vec<usize>>,
    pub negative_watches: Vec<Vec<(usize, usize)>>,
    pub inputs: Vec<bool>,
}

impl Solver {
    pub fn new(intersections: Vec<Vec<usize>>, inputs: Vec<bool>) -> Self {
        let mut solver = Solver {
            intersections: intersections.into_iter().map(|inputs| {
                Intersection {
                    neg_watch_mask: vec![false; inputs.len()],
                    inputs,
                    watch: 0,
                }
            }).collect::<Vec<_>>(),
            positive_watches: Vec::new(),
            negative_watches: Vec::new(),
            inputs,
        };

        solver.positive_watches.reserve(solver.inputs.len());
        solver.negative_watches.reserve(solver.inputs.len());

        for _ in 0..solver.inputs.len() {
            solver.positive_watches.push(Vec::new());
            solver.negative_watches.push(Vec::new());
        }

        for (out, intersection) in solver.intersections.iter_mut().enumerate() {
            let watch = intersection.inputs.iter().position(|&i| !solver.inputs[i]);
            match watch {
                Some(watch) => {
                    intersection.watch = watch;
                    solver.positive_watches[intersection.inputs[watch]].push(out);
                },
                None => {
                    intersection.watch = intersection.inputs.len();
                    for (ix, input) in intersection.inputs.iter().copied().enumerate() {
                        solver.negative_watches[input].push((ix, out));
                    }
                    for neg_watch_flag in intersection.neg_watch_mask.iter_mut() {
                        *neg_watch_flag = true;
                    }
                }
            }
        }

        solver
    }

    pub fn read_true(&mut self, input: usize) -> Vec<usize> {
        self.inputs[input] = true;
        let mut new_outputs = Vec::new();

        dbg!(input, &self.positive_watches);

        for out in std::mem::take(&mut self.positive_watches[input]) {
            let intersection = &mut self.intersections[out];
            let watch = intersection.watch;
            let new_watch = intersection.inputs[watch+1..].iter().position(|&i| !self.inputs[i]);
            match new_watch {
                Some(new_watch) => {
                    let new_watch = watch + 1 + new_watch;
                    self.positive_watches[intersection.inputs[new_watch]].push(out);
                    intersection.watch = new_watch;
                },
                None => {
                    let new_watch =
                        intersection.inputs[..watch].iter().position(|&i| !self.inputs[i]);
                    match new_watch {
                        Some(new_watch) => {
                            self.positive_watches[intersection.inputs[new_watch]].push(out);
                            intersection.watch = new_watch;
                        },
                        None => {
                            intersection.watch = intersection.inputs.len();
                            new_outputs.push(out);
                            for (ix, (input, neg_watch_flag)) in
                                intersection.inputs.iter().copied()
                                .zip(intersection.neg_watch_mask.iter_mut()).enumerate()
                            {
                                if !*neg_watch_flag {
                                    self.negative_watches[input].push((ix, out));
                                    *neg_watch_flag = true;
                                }
                            }
                            for neg_watch_flag in intersection.neg_watch_mask.iter_mut() {
                                *neg_watch_flag = true;
                            }
                        }
                    }
                }
            }
        }

        new_outputs
    }

    pub fn read_false(&mut self, input: usize) -> Vec<usize> {
        self.inputs[input] = false;
        let mut removed_outputs = Vec::new();

        for (ix, out) in std::mem::take(&mut self.negative_watches[input]) {
            let intersection = &mut self.intersections[out];
            intersection.neg_watch_mask[ix] = false;
            if !intersection.is_satisfied() {
                continue;
            }

            intersection.watch = ix;
            removed_outputs.push(out);
            self.positive_watches[intersection.inputs[ix]].push(out);
        }

        removed_outputs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut x: Vec<usize>) -> Vec<usize> {
        x.sort();
        x
    }

    #[test]
    fn it_works() {
        let intersections = vec![
            vec![0, 1],
            vec![1, 2],
            vec![0, 2],
        ];
        let inputs = vec![false, true, false];
        let mut solver = Solver::new(intersections, inputs);
        let no_out: Vec<usize> = Vec::new();

        assert_eq!(sorted(solver.read_true(0)), vec![0]);
        assert_eq!(sorted(solver.read_true(1)), no_out);
        assert_eq!(sorted(solver.read_true(2)), vec![1, 2]);
        assert_eq!(sorted(solver.read_false(0)), vec![0, 2]);
        assert_eq!(sorted(solver.read_false(1)), vec![1]);
        assert_eq!(sorted(solver.read_true(0)), vec![2]);
        assert_eq!(sorted(solver.read_false(0)), vec![2]);
        assert_eq!(sorted(solver.read_true(1)), vec![1]);
        assert_eq!(sorted(solver.read_true(0)), vec![0, 2]);
    }
}
