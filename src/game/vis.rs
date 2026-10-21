pub type V3 = [f32; 3];
pub type Tri = [V3; 3];

const LEAF: usize = 4;

struct Node {
    lo: V3,
    hi: V3,

    at: u32,
    n: u32,
}

pub struct Bvh {
    nodes: Vec<Node>,
    tris: Vec<Tri>,
}

impl Bvh {
    pub fn build(mut tris: Vec<Tri>) -> Bvh {
        tris.retain(|t| t.iter().flatten().all(|v| v.is_finite()));
        let mut b = Bvh { nodes: Vec::with_capacity(tris.len() / 2 + 1), tris: vec![] };
        if !tris.is_empty() {
            let n = tris.len();
            b.split(&mut tris, 0, n);
        }
        b.tris = tris;
        b
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }

    fn split(&mut self, tris: &mut [Tri], first: usize, n: usize) -> usize {
        let me = self.nodes.len();
        let s = &mut tris[first..first + n];
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let (mut clo, mut chi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for t in s.iter() {
            let c = centroid(t);
            for k in 0..3 {
                for v in t {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
                clo[k] = clo[k].min(c[k]);
                chi[k] = chi[k].max(c[k]);
            }
        }
        self.nodes.push(Node { lo, hi, at: first as u32, n: n as u32 });
        if n <= LEAF {
            return me;
        }
        let axis = (0..3).max_by(|&a, &b| (chi[a] - clo[a]).total_cmp(&(chi[b] - clo[b]))).unwrap();
        if chi[axis] - clo[axis] <= 1e-6 {
            return me;
        }
        let half = n / 2;
        s.select_nth_unstable_by(half, |a, b| centroid(a)[axis].total_cmp(&centroid(b)[axis]));
        self.split(tris, first, half);
        let right = self.split(tris, first + half, n - half);
        self.nodes[me].at = right as u32;
        self.nodes[me].n = 0;
        me
    }

    pub fn blocked(&self, a: V3, b: V3) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let inv = d.map(|x| if x.abs() > 1e-12 { 1.0 / x } else { f32::MAX.copysign(x) });
        let mut stack = [0u32; 64];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let i = stack[sp];
            let node = &self.nodes[i as usize];
            if !slab(node, a, inv) {
                continue;
            }
            if node.n > 0 {
                let ts = &self.tris[node.at as usize..(node.at + node.n) as usize];
                if ts.iter().any(|t| hit(t, a, d)) {
                    return true;
                }
            } else if sp + 2 <= stack.len() {
                stack[sp] = node.at;
                stack[sp + 1] = i + 1;
                sp += 2;
            }
        }
        false
    }
}

fn centroid(t: &Tri) -> V3 {
    std::array::from_fn(|k| (t[0][k] + t[1][k] + t[2][k]) / 3.0)
}

fn slab(n: &Node, o: V3, inv: V3) -> bool {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for k in 0..3 {
        let (mut a, mut b) = ((n.lo[k] - o[k]) * inv[k], (n.hi[k] - o[k]) * inv[k]);
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        t0 = t0.max(a);
        t1 = t1.min(b);
        if t0 > t1 {
            return false;
        }
    }
    true
}

fn hit(t: &Tri, o: V3, d: V3) -> bool {
    let sub = |a: V3, b: V3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-9 {
        return false;
    }
    let inv = 1.0 / det;
    let s = sub(o, t[0]);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    let t = dot(e2, q) * inv;
    t > 1e-4 && t < 1.0 - 1e-4
}
