//! vector is the math over the game's world: points and directions in world
//! units (about an inch each), with z up, and view angles in degrees as the
//! game gives them: pitch positive looking down, yaw counterclockwise from +x
//! seen from above, and roll about the view.
//!
//! A vector adds, subtracts and scales with the usual signs, and the rest are
//! methods. Each is a few arithmetic operations on a value copied by value,
//! so a frame can call them freely.

use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

use crate::generated::{Angles, Vector};

impl Vector {
    /// new builds a point or direction from its coordinates.
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// dot is the dot product with other.
    pub fn dot(self, other: Vector) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// length is the vector's length.
    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// normalize is the vector at unit length, or +x for a zero vector.
    pub fn normalize(self) -> Vector {
        let size = self.length();
        if size > 0.0 {
            self * (1.0 / size)
        } else {
            Vector::new(1.0, 0.0, 0.0)
        }
    }

    /// lerp is the point t of the way to other: the vector at 0, other at 1.
    pub fn lerp(self, other: Vector, t: f64) -> Vector {
        self + (other - self) * t
    }

    /// distance is how far the point is from other.
    pub fn distance(self, other: Vector) -> f64 {
        (self - other).length()
    }

    /// flat_distance is how far the point is from other seen from above,
    /// ignoring height.
    pub fn flat_distance(self, other: Vector) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    /// toward is the unit direction from the point to to, or zero when they
    /// meet.
    pub fn toward(self, to: Vector) -> Vector {
        let span = self.distance(to);
        if span > 0.0 {
            (to - self) * (1.0 / span)
        } else {
            Vector::default()
        }
    }

    /// yaw_toward is the yaw that faces from the point toward to, in degrees.
    pub fn yaw_toward(self, to: Vector) -> f64 {
        (to.y - self.y).atan2(to.x - self.x).to_degrees()
    }

    /// aim_at is the view from the point that looks at to, without roll.
    pub fn aim_at(self, to: Vector) -> Angles {
        let across = self.flat_distance(to);
        Angles {
            pitch: -(to.z - self.z).atan2(across).to_degrees(),
            yaw: self.yaw_toward(to),
            roll: 0.0,
        }
    }
}

impl Angles {
    /// new builds a view from its angles, in degrees.
    pub const fn new(pitch: f64, yaw: f64, roll: f64) -> Self {
        Self { pitch, yaw, roll }
    }

    /// level is the view that looks along yaw, straight ahead.
    pub const fn level(yaw: f64) -> Self {
        Self::new(0.0, yaw, 0.0)
    }

    /// forward is the unit direction the view looks along; roll does not
    /// change it.
    pub fn forward(self) -> Vector {
        let (pitch, yaw) = (self.pitch.to_radians(), self.yaw.to_radians());
        Vector {
            x: pitch.cos() * yaw.cos(),
            y: pitch.cos() * yaw.sin(),
            z: -pitch.sin(),
        }
    }
}

impl Add for Vector {
    type Output = Vector;

    fn add(self, other: Vector) -> Vector {
        Vector::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vector {
    type Output = Vector;

    fn sub(self, other: Vector) -> Vector {
        Vector::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f64> for Vector {
    type Output = Vector;

    fn mul(self, scale: f64) -> Vector {
        Vector::new(self.x * scale, self.y * scale, self.z * scale)
    }
}

impl Neg for Vector {
    type Output = Vector;

    fn neg(self) -> Vector {
        self * -1.0
    }
}

impl AddAssign for Vector {
    fn add_assign(&mut self, other: Vector) {
        *self = *self + other;
    }
}

impl SubAssign for Vector {
    fn sub_assign(&mut self, other: Vector) {
        *self = *self - other;
    }
}
