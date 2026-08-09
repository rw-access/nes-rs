#[derive(Debug, Clone)]
pub(crate) struct FirstOrderFilter {
    in_prev: f32,
    out_prev: f32,
    in_weight: f32,
    in_prev_weight: f32,
    out_prev_weight: f32,
}

impl FirstOrderFilter {
    pub(crate) fn filter(&mut self, input: f32) -> f32 {
        self.out_prev = (self.in_prev * self.in_prev_weight)
            + (self.out_prev * self.out_prev_weight)
            + (self.in_weight * input);
        self.in_prev = input;
        self.out_prev
    }

    pub(crate) fn high_pass(sample_rate: f32, freq: f32) -> Self {
        let dt = 1.0 / sample_rate;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * freq);

        Self {
            in_prev: 0.0,
            out_prev: 0.0,
            in_weight: rc / (rc + dt),
            in_prev_weight: -(rc / (rc + dt)),
            out_prev_weight: rc / (rc + dt),
        }
    }

    pub(crate) fn low_pass(sample_rate: f32, freq: f32) -> Self {
        let dt = 1.0 / sample_rate;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * freq);

        Self {
            in_prev: 0.0,
            out_prev: 0.0,
            in_weight: dt / (rc + dt),
            in_prev_weight: 0.0,
            out_prev_weight: 1.0 - (dt / (rc + dt)),
        }
    }
}
