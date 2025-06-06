use std::time::{Duration, Instant};

pub struct CircuitBreaker {
    failure_threshold: u32,
    success_threshold: u32,
    open_duration: u64,
    state: State,
}

enum State {
    Closed { failures: u32 },
    Open { opened_at: Instant },
    HalfOpen { successes: u32 },
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, success_threshold: u32, open_duration: u64) -> Self {
        Self {
            failure_threshold,
            success_threshold,
            open_duration,
            state: State::Closed { failures: 0 },
        }
    }

    pub fn is_closed(&self) -> bool {
        match self.state {
            State::Closed { failures } => true,
            _ => false,
        }
    }

    pub fn should_allow_call(&mut self) -> bool {
        match &self.state {
            State::Closed { .. } => true,
            State::Open { opened_at } => {
                if opened_at.elapsed().as_millis() >= self.open_duration as u128 {
                    self.state = State::HalfOpen { successes: 0 };
                    true
                } else {
                    false
                }
            }
            State::HalfOpen { .. } => true,
        }
    }

    pub fn success(&mut self) {
        match &mut self.state {
            State::Closed { .. } => {}
            State::HalfOpen { successes } => {
                *successes += 1;
                if *successes >= self.success_threshold {
                    self.state = State::Closed { failures: 0 };
                }
            }
            State::Open { .. } => {}
        }
    }

    pub fn fail(&mut self) {
        match &mut self.state {
            State::Closed { failures } => {
                *failures += 1;
                if *failures >= self.failure_threshold {
                    self.state = State::Open {
                        opened_at: Instant::now(),
                    };
                }
            }
            State::HalfOpen { .. } => {
                self.state = State::Open {
                    opened_at: Instant::now(),
                };
            }
            State::Open { .. } => {}
        }
    }
}
