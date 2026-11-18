//! Depth of each file in the dependency order.

use std::collections::BTreeMap;

struct Tarjan<'a> {
    edges: &'a [Vec<usize>],
    index: Vec<Option<usize>>,
    low_link: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    next_index: usize,
    /// Strongly connected components, each listed after every component it reaches.
    components: Vec<Vec<usize>>,
}

impl Tarjan<'_> {
    fn visit(&mut self, node: usize) {
        self.index[node] = Some(self.next_index);
        self.low_link[node] = self.next_index;
        self.next_index += 1;
        self.stack.push(node);
        self.on_stack[node] = true;
        for &next in &self.edges[node] {
            match self.index[next] {
                None => {
                    self.visit(next);
                    self.low_link[node] = self.low_link[node].min(self.low_link[next]);
                }
                Some(next_index) if self.on_stack[next] => {
                    self.low_link[node] = self.low_link[node].min(next_index);
                }
                Some(_) => {}
            }
        }
        if Some(self.low_link[node]) == self.index[node] {
            let mut component = Vec::new();
            while let Some(member) = self.stack.pop() {
                self.on_stack[member] = false;
                component.push(member);
                if member == node {
                    break;
                }
            }
            self.components.push(component);
        }
    }
}

/// Depth 0 for a file that includes nothing in the map, else one more than the deepest
/// file it includes. Files in an include cycle share one depth.
pub fn depths(includes: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, usize> {
    let names: Vec<&String> = includes.keys().collect();
    let position: BTreeMap<&String, usize> = names
        .iter()
        .enumerate()
        .map(|(at, name)| (*name, at))
        .collect();
    let edges: Vec<Vec<usize>> = names
        .iter()
        .map(|name| {
            includes[*name]
                .iter()
                .filter_map(|include| position.get(include))
                .copied()
                .collect()
        })
        .collect();

    let mut tarjan = Tarjan {
        edges: &edges,
        index: vec![None; names.len()],
        low_link: vec![0; names.len()],
        on_stack: vec![false; names.len()],
        stack: Vec::new(),
        next_index: 0,
        components: Vec::new(),
    };
    for node in 0..names.len() {
        if tarjan.index[node].is_none() {
            tarjan.visit(node);
        }
    }

    let mut depth = vec![0; names.len()];
    for component in &tarjan.components {
        let deepest_outside = component
            .iter()
            .flat_map(|&member| edges[member].iter().copied())
            .filter(|next| !component.contains(next))
            .map(|next| depth[next] + 1)
            .max()
            .unwrap_or(0);
        for &member in component {
            depth[member] = deepest_outside;
        }
    }
    names
        .into_iter()
        .zip(depth)
        .map(|(name, depth)| (name.clone(), depth))
        .collect()
}
