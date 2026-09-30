//! Native AnimationState scalar/timing orchestration. Pose evaluation and
//! registered handlers remain synchronous engine services, not timing stubs.
use crate::animation_graph::{Event, Graph, StateInfo, UNKNOWN_STATE};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Playback {
    pub function: u32,
    pub pose_valid: bool,
    pub time: f32,
    pub duration: f32,
    pub speed: f32,
    pub current: usize,
    pub start: f32,
    pub trim: f32,
    pub blend_remaining: f32,
    pub blend_total: f32,
    pub blending: bool,
    pub skip_advance: bool,
    pub auto_after: f32,
    pub pose_words: u32,
    pub marker_count: i32,
    pub handler_count: i32,
}

#[cfg(test)]
#[path = "animation_playback_tests.rs"]
mod tests;
#[derive(Clone, Debug, PartialEq)]
pub enum PoseEffect {
    Copy {
        bytes: u32,
    },
    Still {
        buffer: u8,
        masked: bool,
    },
    Evaluate {
        function: u32,
        time: f32,
        buffer: u8,
        masked: bool,
    },
    Blend {
        weight: f32,
        masked: bool,
    },
    Procedural,
    Skin {
        masked: bool,
    },
    Marker(i32),
}
pub trait Services {
    fn random_index(&mut self, high: usize) -> usize;
    fn random_time(&mut self, high: f32) -> f32;
    fn clip(&mut self, index: usize) -> u32;
    fn allocate(&mut self, clip: u32) -> u32;
    fn release(&mut self, function: u32);
    /// FnAnim virtual UseFPS (+0x10); true selects seconds when supported.
    fn use_fps(&mut self, function: u32, enabled: bool);
    /// FnAnim virtual GetLength (+0x18), in the function's current units.
    fn function_length(&mut self, function: u32) -> f32;
    fn pose(&mut self, state: &Playback, effect: PoseEffect);
    fn has_procedural(&mut self) -> bool;
    /// Native handlers may change playback/handler registration synchronously.
    fn event(&mut self, state: &mut Playback, handler: i32, event: &Event);
}
fn state(graph: &Graph, id: usize) -> Result<&StateInfo, String> {
    graph
        .states
        .get(id)
        .and_then(Option::as_ref)
        .ok_or_else(|| format!("animation state {id} has no loaded metadata"))
}
impl Playback {
    /// SetNextAnim (0x803c8318). No current-ID write: its caller owns that store.
    pub fn select(
        &mut self,
        graph: &Graph,
        id: usize,
        host: &mut impl Services,
    ) -> Result<(), String> {
        let next = state(graph, id)?;
        if next.clips.is_empty() {
            return Err(format!("animation state {id} has no clips"));
        }
        let choice = if next.clips.len() > 1 {
            host.random_index(next.clips.len() - 1)
        } else {
            0
        };
        let index = *next
            .clips
            .get(choice)
            .ok_or("animation RNG exceeded native range")?;
        let clip = host.clip(index);
        self.function = host.allocate(clip);
        self.start = next.start;
        self.trim = next.trim;
        if next.frame_time {
            self.duration = host.function_length(self.function) - 1.;
            if self.auto_after > 0. {
                self.auto_after *= 30.;
            }
            if self.blending {
                self.blend_remaining *= 30.;
                self.blend_total *= 30.;
            }
        } else {
            host.use_fps(self.function, false);
            let samples = host.function_length(self.function);
            host.use_fps(self.function, true);
            let seconds = host.function_length(self.function);
            self.duration = seconds - seconds / samples;
        }
        self.duration -= self.start + self.trim;
        self.time = if next.random {
            host.random_time(self.duration) + self.start
        } else if next.reverse {
            self.duration + self.start
        } else {
            self.start
        };
        Ok(())
    }
    /// SetNextAnimState (0x803c8b78), including outgoing-state blend selection.
    /// Unloaded targets still write speed, then return false as native does.
    pub fn set_next(
        &mut self,
        graph: &Graph,
        id: usize,
        speed: f32,
        force: bool,
        after_ms: i32,
        host: &mut impl Services,
    ) -> Result<bool, String> {
        if id == self.current && !force {
            return Ok(false);
        }
        self.speed = speed;
        let target = graph
            .states
            .get(id)
            .ok_or("animation state ID exceeds native graph")?;
        let Some(_) = target else {
            return Ok(false);
        };
        let blend = state(graph, self.current)?.blend;
        self.current = id;
        self.blend_remaining = blend;
        self.blend_total = blend;
        self.blending = true;
        self.skip_advance = true;
        self.auto_after = (after_ms as f32) / 1000.;
        host.pose(
            self,
            PoseEffect::Copy {
                bytes: self.pose_words.wrapping_mul(4),
            },
        );
        if self.function != 0 {
            host.release(self.function);
            self.function = 0;
        }
        self.select(graph, id, host)?;
        Ok(true)
    }
    pub fn set_state_time(&mut self, graph: &Graph, value: f32) -> Result<(), String> {
        let value = if state(graph, self.current)?.reverse {
            1. - value
        } else {
            value
        };
        self.time = value.mul_add(self.duration, self.start);
        Ok(())
    }
    pub fn state_time(&self, graph: &Graph) -> Result<f32, String> {
        let value = (self.time - self.start) / self.duration;
        Ok(if state(graph, self.current)?.reverse {
            1. - value
        } else {
            value
        })
    }
    /// ProcessAnimEvents (0x803c89a4). Uses the unscaled delta, even when speed
    /// differs from one. Intervals are inclusive at the start, exclusive at end.
    pub fn events(
        &mut self,
        graph: &Graph,
        delta: f32,
        host: &mut impl Services,
    ) -> Result<(), String> {
        let info = state(graph, self.current)?;
        let (begin, end) = if info.reverse {
            let end = self.time;
            let mut begin = end - delta;
            if begin < 0. {
                begin = if info.looping {
                    begin + self.duration
                } else {
                    0.
                };
            }
            (begin, end)
        } else {
            let begin = self.time;
            let mut end = begin + delta;
            if end >= self.duration {
                end = if info.looping {
                    end - self.duration
                } else {
                    self.duration
                };
            }
            (begin, end)
        };
        for event in info.events.iter().flatten() {
            let t = event.time;
            let hit = if begin <= end {
                t >= begin && t < end
            } else {
                (t >= begin && t < self.duration) || (t >= 0. && t < end)
            };
            if hit {
                let mut handler = 0;
                while handler < self.handler_count {
                    host.event(self, handler, event);
                    handler += 1;
                }
            }
        }
        Ok(())
    }
    fn markers(&self, host: &mut impl Services) -> Result<(), String> {
        if self.marker_count > 32 {
            return Err("animation marker count exceeds native 32 slots".into());
        }
        for index in 0..self.marker_count {
            host.pose(self, PoseEffect::Marker(index));
        }
        Ok(())
    }
    /// Complete Update (0x803c851c) timing and ordered pose/marker boundaries.
    pub fn update(
        &mut self,
        graph: &Graph,
        seconds: f32,
        request_pose: bool,
        use_mask: bool,
        host: &mut impl Services,
    ) -> Result<(), String> {
        let calculate_pose = request_pose || !self.pose_valid;
        let masked = calculate_pose && use_mask;
        if self.skip_advance {
            self.skip_advance = false;
        } else {
            let info = state(graph, self.current)?;
            let delta = if info.frame_time {
                30. * seconds
            } else {
                seconds
            };
            self.events(graph, delta, host)?;
            self.time = if info.reverse {
                -delta.mul_add(self.speed, -self.time)
            } else {
                delta.mul_add(self.speed, self.time)
            };
            let ended = if info.reverse {
                self.time <= self.start
            } else {
                self.time >= self.duration + self.start
            };
            if ended {
                let next = info.next;
                if info.looping || (self.auto_after > 0. && next != UNKNOWN_STATE) {
                    self.time = if info.reverse {
                        self.time + self.duration
                    } else {
                        self.time - self.duration
                    };
                } else {
                    self.time = if info.reverse {
                        self.start
                    } else {
                        self.duration + self.start
                    };
                    if next != UNKNOWN_STATE {
                        self.set_next(graph, next, 1., false, -1, host)?;
                    }
                }
            }
            if self.auto_after > 0. {
                let current = state(graph, self.current)?;
                let delta = if current.frame_time {
                    30. * seconds
                } else {
                    seconds
                };
                self.auto_after -= delta;
                if self.auto_after <= 0. {
                    let next = if current.next == UNKNOWN_STATE {
                        0
                    } else {
                        current.next
                    };
                    self.set_next(graph, next, 1., false, -1, host)?;
                }
            }
            self.markers(host)?;
        }
        if self.blending {
            let delta = if state(graph, self.current)?.frame_time {
                30. * seconds
            } else {
                seconds
            };
            self.blend_remaining -= delta;
            if self.blend_remaining <= 0. {
                self.blending = false;
            }
        }
        if calculate_pose {
            if self.blending {
                let weight = 1. - self.blend_remaining / self.blend_total;
                host.pose(
                    self,
                    PoseEffect::Still {
                        buffer: 1,
                        masked: false,
                    },
                );
                host.pose(
                    self,
                    PoseEffect::Evaluate {
                        function: self.function,
                        time: self.time,
                        buffer: 1,
                        masked,
                    },
                );
                host.pose(
                    self,
                    PoseEffect::Still {
                        buffer: 0,
                        masked: false,
                    },
                );
                host.pose(self, PoseEffect::Blend { weight, masked });
            } else {
                host.pose(self, PoseEffect::Still { buffer: 0, masked });
                host.pose(
                    self,
                    PoseEffect::Evaluate {
                        function: self.function,
                        time: self.time,
                        buffer: 0,
                        masked,
                    },
                );
            }
            if host.has_procedural() {
                host.pose(self, PoseEffect::Procedural);
            }
            host.pose(self, PoseEffect::Skin { masked });
            self.pose_valid = true;
        }
        if self.pose_valid {
            self.markers(host)?;
        }
        Ok(())
    }
}
