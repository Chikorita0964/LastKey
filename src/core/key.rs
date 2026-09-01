#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum LogicalKey {
    VerticalFirst = 0,
    VerticalSecond = 1,
    HorizontalFirst = 2,
    HorizontalSecond = 3,
}

impl LogicalKey {
    pub const ALL: [Self; 4] = [
        Self::VerticalFirst,
        Self::VerticalSecond,
        Self::HorizontalFirst,
        Self::HorizontalSecond,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn axis(self) -> Axis {
        match self {
            Self::VerticalFirst | Self::VerticalSecond => Axis::Vertical,
            Self::HorizontalFirst | Self::HorizontalSecond => Axis::Horizontal,
        }
    }

    pub const fn axis_keys(axis: Axis) -> (Self, Self) {
        match axis {
            Axis::Vertical => (Self::VerticalFirst, Self::VerticalSecond),
            Axis::Horizontal => (Self::HorizontalFirst, Self::HorizontalSecond),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyAction {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalKey {
    pub scan_code: u16,
    pub extended: bool,
}
