use crate::robot::models::QueuedRoute;

fn transition_cost<F>(a: &QueuedRoute, b: &QueuedRoute, cost: &F) -> f64
where
    F: Fn(&str, &str) -> f64,
{
    cost(&a.destination, &b.start)
}

fn greedy_atsp_path<F>(mut routes: Vec<QueuedRoute>, cost: F) -> Vec<QueuedRoute>
where
    F: Fn(&str, &str) -> f64,
{
    if routes.len() <= 1 {
        return routes;
    }

    let mut path = Vec::with_capacity(routes.len());

    // Start from the oldest route (arbitrary but stable)
    routes.sort_by_key(|r| r.added_at);
    path.push(routes.remove(0));

    while !routes.is_empty() {
        let last = path.last().unwrap();

        let (best_idx, _) = routes
            .iter()
            .enumerate()
            .map(|(i, r)| (i, transition_cost(last, r, &cost)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap();

        path.push(routes.remove(best_idx));
    }

    path
}

fn two_opt_atsp_path<F>(mut path: Vec<QueuedRoute>, cost: F) -> Vec<QueuedRoute>
where
    F: Fn(&str, &str) -> f64,
{
    let n = path.len();
    if n < 4 {
        return path;
    }

    let mut improved = true;

    while improved {
        improved = false;

        for i in 0..n - 2 {
            for j in i + 2..n {
                let a = &path[i];
                let b = &path[i + 1];
                let c = &path[j - 1];
                let d = &path[j];

                let current = transition_cost(a, b, &cost) + transition_cost(c, d, &cost);

                let swapped = transition_cost(a, c, &cost) + transition_cost(b, d, &cost);

                if swapped < current {
                    path[i + 1..j].reverse();
                    improved = true;
                }
            }
        }
    }

    path
}

pub fn solve_atsp_path<F>(routes: Vec<QueuedRoute>, cost: F) -> Vec<QueuedRoute>
where
    F: Fn(&str, &str) -> f64,
{
    let greedy = greedy_atsp_path(routes, &cost);
    two_opt_atsp_path(greedy, cost)
}
