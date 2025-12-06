use chrono::{Duration as ChronoDuration, NaiveDateTime, Utc};
use rand::distributions::{Alphanumeric, DistString};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rand_distr::{Distribution as RandDistribution, Zipf};

use crate::config::Distribution;

#[derive(Clone, Debug)]
pub struct EventRow {
    pub user_id: i64,
    pub created_at: NaiveDateTime,
    pub amount: f64,
    pub status: i16,
    pub category: i32,
    pub payload: String,
}

pub struct EventGenerator {
    rng: StdRng,
    payload_size: usize,
    distribution: Distribution,
    zipf: Option<Zipf<f64>>,
}

impl EventGenerator {
    pub fn new(distribution: Distribution, payload_size: usize) -> Self {
        let rng = StdRng::from_entropy();
        let zipf = if matches!(distribution, Distribution::Zipf) {
            Some(Zipf::new(1_000_000, 1.03).expect("zipf parameters valid"))
        } else {
            None
        };

        Self {
            rng,
            payload_size,
            distribution,
            zipf,
        }
    }

    pub fn with_seed(distribution: Distribution, payload_size: usize, seed: u64) -> Self {
        let rng = StdRng::seed_from_u64(seed);
        let zipf = if matches!(distribution, Distribution::Zipf) {
            Some(Zipf::new(1_000_000, 1.03).expect("zipf parameters valid"))
        } else {
            None
        };

        Self {
            rng,
            payload_size,
            distribution,
            zipf,
        }
    }

    pub fn next_batch(&mut self, size: usize) -> Vec<EventRow> {
        (0..size).map(|_| self.next_row()).collect()
    }

    fn next_row(&mut self) -> EventRow {
        let user_id = self.sample_user_id();
        let now = Utc::now().naive_utc();
        let created_at = now - ChronoDuration::seconds(self.rng.gen_range(0..(30 * 24 * 3600)));
        let amount = (self.rng.gen_range(0.0f64..1000.0f64) * 100.0f64).round() / 100.0f64;
        let status = self.rng.gen_range(0..5) as i16;
        let category = self.rng.gen_range(0..=5000) as i32;
        let payload = Alphanumeric.sample_string(&mut self.rng, self.payload_size);

        EventRow {
            user_id,
            created_at,
            amount,
            status,
            category,
            payload,
        }
    }

    fn sample_user_id(&mut self) -> i64 {
        match (self.distribution, &self.zipf) {
            (Distribution::Zipf, Some(zipf)) => zipf.sample(&mut self.rng) as i64,
            _ => self.rng.gen_range(1..=1_000_000),
        }
    }
}
