use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Debug, Clone)]
pub struct McmfEdge {
    pub to: usize,
    pub rev: usize,
    pub cap: i64,
    pub cost: i64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct HeapState {
    cost: i64,
    node: usize,
}

impl Ord for HeapState {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap behavior via reversed ordering.
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| self.node.cmp(&other.node))
    }
}

impl PartialOrd for HeapState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn min_cost_max_flow(
    graph: &mut [Vec<McmfEdge>],
    source: usize,
    sink: usize,
    max_flow: i64,
) -> (i64, i64) {
    let node_count = graph.len();
    let mut potentials = vec![0i64; node_count];
    let mut total_flow = 0i64;
    let mut total_cost = 0i64;

    let mut dist = vec![0i64; node_count];
    let mut prev_node = vec![0usize; node_count];
    let mut prev_edge = vec![0usize; node_count];

    while total_flow < max_flow {
        dist.fill(i64::MAX / 4);
        dist[source] = 0;

        let mut heap = BinaryHeap::new();
        heap.push(HeapState {
            cost: 0,
            node: source,
        });

        while let Some(HeapState { cost, node }) = heap.pop() {
            if cost != dist[node] {
                continue;
            }

            for (edge_idx, edge) in graph[node].iter().enumerate() {
                if edge.cap <= 0 {
                    continue;
                }

                let next = edge.to;
                let next_cost = cost + edge.cost + potentials[node] - potentials[next];
                if next_cost < dist[next] {
                    dist[next] = next_cost;
                    prev_node[next] = node;
                    prev_edge[next] = edge_idx;
                    heap.push(HeapState {
                        cost: next_cost,
                        node: next,
                    });
                }
            }
        }

        if dist[sink] >= i64::MAX / 5 {
            break;
        }

        for node in 0..node_count {
            if dist[node] < i64::MAX / 5 {
                potentials[node] += dist[node];
            }
        }

        let mut add_flow = max_flow - total_flow;
        let mut v = sink;
        while v != source {
            let u = prev_node[v];
            let edge_idx = prev_edge[v];
            let cap = graph[u][edge_idx].cap;
            add_flow = add_flow.min(cap);
            v = u;
        }

        v = sink;
        while v != source {
            let u = prev_node[v];
            let edge_idx = prev_edge[v];
            let rev = graph[u][edge_idx].rev;

            graph[u][edge_idx].cap -= add_flow;
            graph[v][rev].cap += add_flow;

            total_cost += graph[u][edge_idx].cost * add_flow;
            v = u;
        }

        total_flow += add_flow;
    }

    (total_flow, total_cost)
}

pub fn add_edge(graph: &mut [Vec<McmfEdge>], from: usize, to: usize, cap: i64, cost: i64) {
    let from_rev = graph[to].len();
    let to_rev = graph[from].len();

    graph[from].push(McmfEdge {
        to,
        rev: from_rev,
        cap,
        cost,
    });
    graph[to].push(McmfEdge {
        to: from,
        rev: to_rev,
        cap: 0,
        cost: -cost,
    });
}
