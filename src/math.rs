use std::f32::consts::FRAC_PI_4;

pub type Vector = [f32; 3];

pub fn sub(a: Vector, b: Vector) -> Vector { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }

pub fn dot(a: Vector, b: Vector) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }

pub fn cross(a: Vector, b: Vector) -> Vector
{
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn length(v: Vector) -> f32 { dot(v, v).sqrt() }

pub fn normalize(v: Vector) -> Vector
{
    let length: f32 = length(v);
    if length > 0.0
    {
        [v[0] / length, v[1] / length, v[2] / length]
    }
    else
    {
        v
    }
}

pub fn lerp(from: f32, to: f32, fraction: f32) -> f32 { from + (to - from) * fraction }

pub fn equalPowerPan(pan: f32) -> [f32; 2]
{
    let angle: f32 = (pan + 1.0) * FRAC_PI_4;
    [angle.cos(), angle.sin()]
}

#[derive(Clone, Copy)]
pub struct Ramp
{
    m_value: f32,
    m_target: f32,
    m_step: f32,
}

impl Ramp
{
    pub fn new(value: f32) -> Ramp { Ramp { m_value: value, m_target: value, m_step: 0.0 } }

    pub fn toward(&mut self, target: f32, frames: f32)
    {
        self.m_target = target;
        if frames < 1.0
        {
            self.m_value = target;
            self.m_step = 0.0;
            return;
        }

        self.m_step = (target - self.m_value) / frames;
    }

    pub fn settled(&self) -> bool { self.m_value == self.m_target }

    // returns the value at the start and the end of the block
    pub fn advance(&mut self, frames: usize) -> [f32; 2]
    {
        let start: f32 = self.m_value;
        let moved: f32 = self.m_value + self.m_step * frames as f32;
        let passed: bool = if self.m_step > 0.0
        {
            moved >= self.m_target
        }
        else
        {
            moved <= self.m_target
        };
        self.m_value = if passed { self.m_target } else { moved };
        [start, self.m_value]
    }
}
