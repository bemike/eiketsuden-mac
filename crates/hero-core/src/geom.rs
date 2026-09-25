//! Grid geometry: positions, directions and distances.

use serde::{Deserialize, Serialize};

/// A tile position. Serialized as a two-element array `[x, y]` so data files stay compact.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(from = "[i32; 2]", into = "[i32; 2]")]
pub struct Pos {
    pub x: i32,
    pub y: i32,
}

impl From<[i32; 2]> for Pos {
    fn from(a: [i32; 2]) -> Self {
        Pos { x: a[0], y: a[1] }
    }
}

impl From<Pos> for [i32; 2] {
    fn from(p: Pos) -> Self {
        [p.x, p.y]
    }
}

impl Pos {
    pub const fn new(x: i32, y: i32) -> Self {
        Pos { x, y }
    }

    pub fn offset(self, dx: i32, dy: i32) -> Pos {
        Pos::new(self.x + dx, self.y + dy)
    }

    pub fn manhattan(self, o: Pos) -> i32 {
        (self.x - o.x).abs() + (self.y - o.y).abs()
    }

    pub fn chebyshev(self, o: Pos) -> i32 {
        (self.x - o.x).abs().max((self.y - o.y).abs())
    }

    /// The four orthogonal neighbours (up, down, left, right).
    pub fn neighbors4(self) -> [Pos; 4] {
        [
            self.offset(0, -1),
            self.offset(0, 1),
            self.offset(-1, 0),
            self.offset(1, 0),
        ]
    }

    /// The eight surrounding tiles.
    pub fn neighbors8(self) -> [Pos; 8] {
        [
            self.offset(-1, -1),
            self.offset(0, -1),
            self.offset(1, -1),
            self.offset(-1, 0),
            self.offset(1, 0),
            self.offset(-1, 1),
            self.offset(0, 1),
            self.offset(1, 1),
        ]
    }
}

/// Facing direction of a unit sprite.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dir {
    Up,
    #[default]
    Down,
    Left,
    Right,
}

impl Dir {
    /// Direction that best points from `from` towards `to` (horizontal wins ties).
    pub fn towards(from: Pos, to: Pos) -> Dir {
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        if dx == 0 && dy == 0 {
            Dir::Down
        } else if dx.abs() >= dy.abs() {
            if dx > 0 {
                Dir::Right
            } else {
                Dir::Left
            }
        } else if dy > 0 {
            Dir::Down
        } else {
            Dir::Up
        }
    }

    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::Up => (0, -1),
            Dir::Down => (0, 1),
            Dir::Left => (-1, 0),
            Dir::Right => (1, 0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances() {
        let a = Pos::new(1, 1);
        let b = Pos::new(4, -1);
        assert_eq!(a.manhattan(b), 5);
        assert_eq!(a.chebyshev(b), 3);
    }

    #[test]
    fn serde_as_array() {
        let p: Pos = serde_json::from_str("[3, 7]").unwrap();
        assert_eq!(p, Pos::new(3, 7));
        assert_eq!(serde_json::to_string(&p).unwrap(), "[3,7]");
    }

    #[test]
    fn towards() {
        let o = Pos::new(5, 5);
        assert_eq!(Dir::towards(o, Pos::new(9, 6)), Dir::Right);
        assert_eq!(Dir::towards(o, Pos::new(5, 1)), Dir::Up);
        assert_eq!(Dir::towards(o, Pos::new(4, 5)), Dir::Left);
        assert_eq!(Dir::towards(o, Pos::new(5, 8)), Dir::Down);
    }
}
