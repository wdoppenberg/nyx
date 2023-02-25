/*
    Nyx, blazing fast astrodynamics
    Copyright (C) 2023 Christopher Rabotin <christopher.rabotin@gmail.com>

    This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU Affero General Public License as published
    by the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU Affero General Public License for more details.

    You should have received a copy of the GNU Affero General Public License
    along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use super::traj_it::TrajIterator;
use super::INTERPOLATION_SAMPLES;
use super::{InterpState, TrajError};
use crate::cosmic::{Cosm, Frame, Orbit, Spacecraft};
use crate::errors::NyxError;
use crate::io::formatter::StateFormatter;
use crate::io::watermark::pq_writer;
use crate::linalg::allocator::Allocator;
use crate::linalg::DefaultAllocator;
use crate::md::StateParameter;
use crate::md::{events::EventEvaluator, MdHdlr, OrbitStateOutput};
use crate::time::{Duration, Epoch, TimeSeries, Unit};
use crate::State;
use arrow::array::{ArrayRef, Float64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use rayon::prelude::*;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::iter::Iterator;
use std::ops;
use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Instant;

/// Store a trajectory of any State.
#[derive(Clone)]
pub struct Traj<S: InterpState>
where
    DefaultAllocator:
        Allocator<f64, S::VecLength> + Allocator<f64, S::Size> + Allocator<f64, S::Size, S::Size>,
{
    /// Optionally name this trajectory
    pub name: Option<String>,
    /// We use a vector because we know that the states are produced in a chronological manner (the direction does not matter).
    pub states: Vec<S>,
}

impl<S: InterpState> Default for Traj<S>
where
    DefaultAllocator:
        Allocator<f64, S::VecLength> + Allocator<f64, S::Size> + Allocator<f64, S::Size, S::Size>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S: InterpState> Traj<S>
where
    DefaultAllocator:
        Allocator<f64, S::VecLength> + Allocator<f64, S::Size> + Allocator<f64, S::Size, S::Size>,
{
    pub fn new() -> Self {
        Self {
            name: None,
            states: Vec::new(),
        }
    }
    /// Orders the states, can be used to store the states out of order
    pub fn finalize(&mut self) {
        // Remove duplicate epochs
        self.states.dedup_by(|a, b| a.epoch().eq(&b.epoch()));
        // And sort
        self.states.sort_by_key(|a| a.epoch());
    }

    /// Evaluate the trajectory at this specific epoch.
    pub fn at(&self, epoch: Epoch) -> Result<S, NyxError> {
        if self.states.is_empty() || self.first().epoch() > epoch || self.last().epoch() < epoch {
            return Err(NyxError::Trajectory(TrajError::NoInterpolationData(epoch)));
        }
        match self
            .states
            .binary_search_by(|state| state.epoch().cmp(&epoch))
        {
            Ok(idx) => {
                // Oh wow, we actually had this exact state!
                Ok(self.states[idx])
            }
            Err(idx) => {
                if idx == 0 || idx >= self.states.len() {
                    // The binary search returns where we should insert the data, so if it's at either end of the list, then we're out of bounds.
                    // This condition should have been handled by the check at the start of this function.
                    return Err(NyxError::Trajectory(TrajError::NoInterpolationData(epoch)));
                }
                // This is the closest index, so let's grab the items around it.
                // NOTE: This is essentially the same code as in ANISE for the Hermite SPK type 13

                // We didn't find it, so let's build an interpolation here.
                let num_left = INTERPOLATION_SAMPLES / 2;

                // Ensure that we aren't fetching out of the window
                let mut first_idx = idx.saturating_sub(num_left);
                let last_idx = self.states.len().min(first_idx + INTERPOLATION_SAMPLES);

                // Check that we have enough samples
                if last_idx == self.states.len() {
                    first_idx = last_idx - 2 * num_left;
                }

                let mut states = Vec::with_capacity(last_idx - first_idx);
                for idx in first_idx..last_idx {
                    states.push(self.states[idx]);
                }

                self.states[idx].interpolate(epoch, &states)
            }
        }
    }

    /// Returns the first state in this ephemeris
    pub fn first(&self) -> &S {
        // This is done after we've ordered the states we received, so we can just return the first state.
        self.states.first().unwrap()
    }

    /// Returns the last state in this ephemeris
    pub fn last(&self) -> &S {
        self.states.last().unwrap()
    }

    /// Creates an iterator through the trajectory by the provided step size
    pub fn every(&self, step: Duration) -> TrajIterator<S> {
        self.every_between(step, self.first().epoch(), self.last().epoch())
    }

    /// Creates an iterator through the trajectory by the provided step size between the provided bounds
    pub fn every_between(&self, step: Duration, start: Epoch, end: Epoch) -> TrajIterator<S> {
        TrajIterator {
            time_series: TimeSeries::inclusive(start, end, step),
            traj: self,
        }
    }

    /// Find the exact state where the request event happens. The event function is expected to be monotone in the provided interval because we find the event using a Brent solver.
    #[allow(clippy::identity_op)]
    pub fn find_bracketed<E>(&self, start: Epoch, end: Epoch, event: &E) -> Result<S, NyxError>
    where
        E: EventEvaluator<S>,
    {
        let max_iter = 50;

        // Helper lambdas, for f64s only
        let has_converged =
            |x1: f64, x2: f64| (x1 - x2).abs() <= event.epoch_precision().to_seconds();
        let arrange = |a: f64, ya: f64, b: f64, yb: f64| {
            if ya.abs() > yb.abs() {
                (a, ya, b, yb)
            } else {
                (b, yb, a, ya)
            }
        };

        let xa_e = start;
        let xb_e = end;

        // Search in seconds (convert to epoch just in time)
        let mut xa = 0.0;
        let mut xb = (xb_e - xa_e).to_seconds();
        // Evaluate the event at both bounds
        let mut ya = event.eval(&self.at(xa_e)?);
        let mut yb = event.eval(&self.at(xb_e)?);

        // Check if we're already at the root
        if ya.abs() <= event.value_precision().abs() {
            return self.at(xa_e);
        } else if yb.abs() <= event.value_precision().abs() {
            return self.at(xb_e);
        }
        // The Brent solver, from the roots crate (sadly could not directly integrate it here)
        // Source: https://docs.rs/roots/0.0.5/src/roots/numerical/brent.rs.html#57-131

        let (mut xc, mut yc, mut xd) = (xa, ya, xa);
        let mut flag = true;

        for _ in 0..max_iter {
            if ya.abs() < event.value_precision().abs() {
                return self.at(xa_e + xa * Unit::Second);
            }
            if yb.abs() < event.value_precision().abs() {
                return self.at(xa_e + xb * Unit::Second);
            }
            if has_converged(xa, xb) {
                // The event isn't in the bracket
                return Err(NyxError::from(TrajError::EventNotFound {
                    start,
                    end,
                    event: format!("{event}"),
                }));
            }
            let mut s = if (ya - yc).abs() > f64::EPSILON && (yb - yc).abs() > f64::EPSILON {
                xa * yb * yc / ((ya - yb) * (ya - yc))
                    + xb * ya * yc / ((yb - ya) * (yb - yc))
                    + xc * ya * yb / ((yc - ya) * (yc - yb))
            } else {
                xb - yb * (xb - xa) / (yb - ya)
            };
            let cond1 = (s - xb) * (s - (3.0 * xa + xb) / 4.0) > 0.0;
            let cond2 = flag && (s - xb).abs() >= (xb - xc).abs() / 2.0;
            let cond3 = !flag && (s - xb).abs() >= (xc - xd).abs() / 2.0;
            let cond4 = flag && has_converged(xb, xc);
            let cond5 = !flag && has_converged(xc, xd);
            if cond1 || cond2 || cond3 || cond4 || cond5 {
                s = (xa + xb) / 2.0;
                flag = true;
            } else {
                flag = false;
            }
            let next_try = self.at(xa_e + s * Unit::Second)?;
            let ys = event.eval(&next_try);
            xd = xc;
            xc = xb;
            yc = yb;
            if ya * ys < 0.0 {
                // Root bracketed between a and s
                let next_try = self.at(xa_e + xa * Unit::Second)?;
                let ya_p = event.eval(&next_try);
                let (_a, _ya, _b, _yb) = arrange(xa, ya_p, s, ys);
                {
                    xa = _a;
                    ya = _ya;
                    xb = _b;
                    yb = _yb;
                }
            } else {
                // Root bracketed between s and b
                let next_try = self.at(xa_e + xb * Unit::Second)?;
                let yb_p = event.eval(&next_try);
                let (_a, _ya, _b, _yb) = arrange(s, ys, xb, yb_p);
                {
                    xa = _a;
                    ya = _ya;
                    xb = _b;
                    yb = _yb;
                }
            }
        }
        Err(NyxError::MaxIterReached(format!(
            "Brent solver failed after {max_iter} iterations",
        )))
    }

    /// Find (usually) all of the states where the event happens.
    /// WARNING: The initial search step is 1% of the duration of the trajectory duration!
    /// For example, if the trajectory is 100 days long, then we split the trajectory into 100 chunks of 1 day and see whether
    /// the event is in there. If the event happens twice or more times within 1% of the trajectory duration, only the _one_ of
    /// such events will be found.
    #[allow(clippy::identity_op)]
    pub fn find_all<E>(&self, event: &E) -> Result<Vec<S>, NyxError>
    where
        E: EventEvaluator<S>,
    {
        let start_epoch = self.first().epoch();
        let end_epoch = self.last().epoch();
        if start_epoch == end_epoch {
            return Err(NyxError::from(TrajError::EventNotFound {
                start: start_epoch,
                end: end_epoch,
                event: format!("{event}"),
            }));
        }
        let heuristic = (end_epoch - start_epoch) / 100;
        info!("Searching for {event} with initial heuristic of {heuristic}",);

        let (sender, receiver) = channel();

        let epochs: Vec<Epoch> = TimeSeries::inclusive(start_epoch, end_epoch, heuristic).collect();
        epochs.into_par_iter().for_each_with(sender, |s, epoch| {
            if let Ok(event_state) = self.find_bracketed(epoch, epoch + heuristic, event) {
                s.send(event_state).unwrap()
            };
        });

        let mut states: Vec<_> = receiver.iter().collect();

        if states.is_empty() {
            warn!("Heuristic failed to find any {event} event, using slower approach");
            // Crap, we didn't find the event.
            // Let's find the min and max of this event throughout the trajectory, and search around there.
            match self.find_minmax(event, Unit::Second) {
                Ok((min_event, max_event)) => {
                    let lower_min_epoch =
                        if min_event.epoch() - 1 * Unit::Millisecond < self.first().epoch() {
                            self.first().epoch()
                        } else {
                            min_event.epoch() - 1 * Unit::Millisecond
                        };

                    let lower_max_epoch =
                        if min_event.epoch() + 1 * Unit::Millisecond > self.last().epoch() {
                            self.last().epoch()
                        } else {
                            min_event.epoch() + 1 * Unit::Millisecond
                        };

                    let upper_min_epoch =
                        if max_event.epoch() - 1 * Unit::Millisecond < self.first().epoch() {
                            self.first().epoch()
                        } else {
                            max_event.epoch() - 1 * Unit::Millisecond
                        };

                    let upper_max_epoch =
                        if max_event.epoch() + 1 * Unit::Millisecond > self.last().epoch() {
                            self.last().epoch()
                        } else {
                            max_event.epoch() + 1 * Unit::Millisecond
                        };

                    // Search around the min event
                    if let Ok(event_state) =
                        self.find_bracketed(lower_min_epoch, lower_max_epoch, event)
                    {
                        states.push(event_state);
                    };

                    // Search around the max event
                    if let Ok(event_state) =
                        self.find_bracketed(upper_min_epoch, upper_max_epoch, event)
                    {
                        states.push(event_state);
                    };

                    // If there still isn't any match, report that the event was not found
                    if states.is_empty() {
                        return Err(NyxError::from(TrajError::EventNotFound {
                            start: start_epoch,
                            end: end_epoch,
                            event: format!("{event}"),
                        }));
                    }
                }
                Err(_) => {
                    return Err(NyxError::from(TrajError::EventNotFound {
                        start: start_epoch,
                        end: end_epoch,
                        event: format!("{event}"),
                    }));
                }
            };
        }
        // Remove duplicates and reorder
        states.sort_by(|s1, s2| s1.epoch().partial_cmp(&s2.epoch()).unwrap());
        states.dedup();
        for (cnt, event_state) in states.iter().enumerate() {
            info!("{} #{}: {}", event, cnt + 1, event_state);
        }
        Ok(states)
    }

    /// Find the minimum and maximum of the provided event through the trajectory
    #[allow(clippy::identity_op)]
    pub fn find_minmax<E>(&self, event: &E, precision: Unit) -> Result<(S, S), NyxError>
    where
        E: EventEvaluator<S>,
    {
        let step: Duration = 1 * precision;
        let mut min_val = std::f64::INFINITY;
        let mut max_val = std::f64::NEG_INFINITY;
        let mut min_state = S::zeros();
        let mut max_state = S::zeros();

        let (sender, receiver) = channel();

        let epochs: Vec<Epoch> =
            TimeSeries::inclusive(self.first().epoch(), self.last().epoch(), step).collect();

        epochs.into_par_iter().for_each_with(sender, |s, epoch| {
            let state = self.at(epoch).unwrap();
            let this_eval = event.eval(&state);
            s.send((this_eval, state)).unwrap();
        });

        let evald_states: Vec<_> = receiver.iter().collect();
        for (this_eval, state) in evald_states {
            if this_eval < min_val {
                min_val = this_eval;
                min_state = state;
            }
            if this_eval > max_val {
                max_val = this_eval;
                max_state = state;
            }
        }

        Ok((min_state, max_state))
    }

    /// Store this tracking arc to a parquet file
    pub fn to_parquet<P: AsRef<Path>>(
        &self,
        path: P,
        additional_fields: Option<Vec<StateParameter>>,
    ) -> Result<P, Box<dyn Error>> {
        let mut fields = vec![
            StateParameter::X,
            StateParameter::Y,
            StateParameter::Z,
            StateParameter::VX,
            StateParameter::VY,
            StateParameter::VZ,
        ];

        if let Some(mut additional_fields) = additional_fields {
            fields.append(&mut additional_fields);
        }

        // Build the schema
        // TODO: Add the custom headers and frame conversions, etc. from state formatter
        let mut hdrs = vec![
            Field::new("Epoch:Gregorian UTC", DataType::Utf8, false),
            Field::new("Epoch:Gregorian TDB", DataType::Utf8, false),
            Field::new("Epoch:TDB (s)", DataType::Float64, false),
        ];

        for field in &fields {
            hdrs.push(field.field());
        }

        // Build the schema
        let schema = Arc::new(Schema::new(hdrs));
        let mut record = Vec::new();

        // Build all of the records
        record.push(Arc::new(StringArray::from(
            self.states
                .iter()
                .map(|s| format!("{}", s.epoch()))
                .collect::<Vec<String>>(),
        )) as ArrayRef);

        // TDB epoch
        record.push(Arc::new(StringArray::from(
            self.states
                .iter()
                .map(|s| format!("{:e}", s.epoch()))
                .collect::<Vec<String>>(),
        )) as ArrayRef);

        // TDB Epoch seconds
        record.push(Arc::new(Float64Array::from(
            self.states
                .iter()
                .map(|s| s.epoch().to_tdb_seconds())
                .collect::<Vec<f64>>(),
        )) as ArrayRef);

        // Add all of the fields

        for field in fields {
            record.push(Arc::new(Float64Array::from(
                self.states
                    .iter()
                    .map(|s| s.value(&field).unwrap())
                    .collect::<Vec<f64>>(),
            )) as ArrayRef);
        }

        // Serialize all of the devices and add that to the parquet file too.
        let mut metadata = HashMap::new();
        metadata.insert("Purpose".to_string(), "Trajectory data".to_string());
        // TODO: Add mission phases here or whatever events are passed as an input

        let props = pq_writer(Some(metadata));
        let file = File::create(&path)?;
        let mut writer = ArrowWriter::try_new(file, schema.clone(), props).unwrap();

        let batch = RecordBatch::try_new(schema, record)?;
        writer.write(&batch)?;
        writer.close()?;

        // Return the path this was written to
        Ok(path)
    }
}

impl<S: InterpState> ops::Add for Traj<S>
where
    DefaultAllocator:
        Allocator<f64, S::VecLength> + Allocator<f64, S::Size> + Allocator<f64, S::Size, S::Size>,
{
    type Output = Traj<S>;

    /// Add one trajectory to another. If they do not overlap to within 10ms, a warning will be printed.
    fn add(self, other: Traj<S>) -> Self::Output {
        self + &other
    }
}

impl<S: InterpState> ops::Add<&Traj<S>> for Traj<S>
where
    DefaultAllocator:
        Allocator<f64, S::VecLength> + Allocator<f64, S::Size> + Allocator<f64, S::Size, S::Size>,
{
    type Output = Traj<S>;

    /// Add one trajectory to another. If they do not overlap to within 10ms, a warning will be printed.
    fn add(self, other: &Traj<S>) -> Self::Output {
        let (first, second) = if self.first().epoch() < other.first().epoch() {
            (&self, other)
        } else {
            (other, &self)
        };

        if first.last().epoch() < second.first().epoch() {
            let gap = second.first().epoch() - first.last().epoch();
            warn!(
                "Resulting merged trajectory will have a time-gap of {} starting at {}",
                gap,
                first.last().epoch()
            );
        }

        let mut me = self.clone();
        // Now start adding the other segments while correcting the index
        for state in &second.states {
            me.states.push(*state);
        }
        me.finalize();
        me
    }
}

impl<S: InterpState> ops::AddAssign for Traj<S>
where
    DefaultAllocator:
        Allocator<f64, S::VecLength> + Allocator<f64, S::Size> + Allocator<f64, S::Size, S::Size>,
{
    fn add_assign(&mut self, rhs: Self) {
        *self = self.clone() + rhs;
    }
}

impl<S: InterpState> ops::AddAssign<&Traj<S>> for Traj<S>
where
    DefaultAllocator:
        Allocator<f64, S::VecLength> + Allocator<f64, S::Size> + Allocator<f64, S::Size, S::Size>,
{
    fn add_assign(&mut self, rhs: &Self) {
        *self = self.clone() + rhs;
    }
}

impl Traj<Orbit> {
    /// Allows converting the source trajectory into the (almost) equivalent trajectory in another frame.
    /// This simply converts each state into the other frame and may lead to aliasing due to the Nyquist–Shannon sampling theorem.
    #[allow(clippy::map_clone)]
    pub fn to_frame(&self, new_frame: Frame, cosm: Arc<Cosm>) -> Result<Self, NyxError> {
        if self.states.is_empty() {
            return Err(NyxError::Trajectory(TrajError::CreationError(
                "No trajectory to convert".to_string(),
            )));
        }
        let start_instant = Instant::now();
        let mut traj = Self::new();
        // For each state, add the state and the one in between two successive states
        for these_states in self.states.windows(2) {
            let state_1 = cosm.frame_chg(&these_states[0], new_frame);
            traj.states.push(state_1);
            let next_epoch = state_1.epoch() + (these_states[1].epoch() - state_1.epoch()) * 0.5;
            match self.at(next_epoch) {
                Ok(intermediate) => {
                    // The at() call will fail on the last state.
                    let state_2 = cosm.frame_chg(&intermediate, new_frame);
                    traj.states.push(state_2);
                }
                Err(e) => error!("{e} @ {next_epoch}"),
            }
        }
        // Add the final state
        let state_2 = cosm.frame_chg(self.last(), new_frame);
        traj.states.push(state_2);
        traj.finalize();

        info!(
            "Converted trajectory from {} to {} in {} ms: {traj}",
            self.first().frame,
            new_frame,
            (Instant::now() - start_instant).as_millis()
        );
        Ok(traj)
    }

    /// Exports this trajectory to the provided filename in CSV format with the default headers and the provided step
    pub fn to_csv_with_step(
        &self,
        filename: &str,
        step: Duration,
        cosm: Arc<Cosm>,
    ) -> Result<(), NyxError> {
        let fmtr = StateFormatter::default(filename.to_string(), cosm);
        let mut out = OrbitStateOutput::new(fmtr)?;
        for state in self.every(step) {
            out.handle(&state);
        }
        Ok(())
    }

    /// Exports this trajectory to the provided filename in CSV format with the default headers and the provided step
    pub fn to_csv_between_with_step(
        &self,
        filename: &str,
        start: Option<Epoch>,
        end: Option<Epoch>,
        step: Duration,
        cosm: Arc<Cosm>,
    ) -> Result<(), NyxError> {
        let fmtr = StateFormatter::default(filename.to_string(), cosm);
        let mut out = OrbitStateOutput::new(fmtr)?;
        let start = match start {
            Some(s) => s,
            None => self.first().epoch(),
        };
        let end = match end {
            Some(e) => e,
            None => self.last().epoch(),
        };
        for state in self.every_between(step, start, end) {
            out.handle(&state);
        }
        Ok(())
    }

    /// Exports this trajectory to the provided filename in CSV format with the default headers, one state per minute
    #[allow(clippy::identity_op)]
    pub fn to_csv(&self, filename: &str, cosm: Arc<Cosm>) -> Result<(), NyxError> {
        self.to_csv_with_step(filename, 1 * Unit::Minute, cosm)
    }

    /// Exports this trajectory to the provided filename in CSV format with the default headers, one state per minute
    #[allow(clippy::identity_op)]
    pub fn to_csv_between(
        &self,
        filename: &str,
        start: Option<Epoch>,
        end: Option<Epoch>,
        cosm: Arc<Cosm>,
    ) -> Result<(), NyxError> {
        self.to_csv_between_with_step(filename, start, end, 1 * Unit::Minute, cosm)
    }

    /// Exports this trajectory to the provided filename in CSV format with only the epoch, the geodetic latitude, longitude, and height at one state per minute.
    /// Must provide a body fixed frame to correctly compute the latitude and longitude.
    #[allow(clippy::identity_op)]
    pub fn to_groundtrack_csv(
        &self,
        filename: &str,
        body_fixed_frame: Frame,
        cosm: Arc<Cosm>,
    ) -> Result<(), NyxError> {
        let fmtr = StateFormatter::from_headers(
            vec![
                "epoch",
                "geodetic_latitude",
                "geodetic_longitude",
                "geodetic_height",
            ],
            filename.to_string(),
            cosm.clone(),
        )?;
        let mut out = OrbitStateOutput::new(fmtr)?;
        for state in self
            .to_frame(body_fixed_frame, cosm)?
            .every(1 * Unit::Minute)
        {
            out.handle(&state);
        }
        Ok(())
    }

    /// Exports this trajectory to the provided filename in CSV format with the provided headers and the provided step
    pub fn to_csv_custom(
        &self,
        filename: &str,
        headers: Vec<&str>,
        step: Duration,
        cosm: Arc<Cosm>,
    ) -> Result<(), NyxError> {
        let fmtr = StateFormatter::from_headers(headers, filename.to_string(), cosm)?;
        let mut out = OrbitStateOutput::new(fmtr)?;
        for state in self.every(step) {
            out.handle(&state);
        }
        Ok(())
    }
}

impl Traj<Spacecraft> {
    /// Allows converting the source trajectory into the (almost) equivalent trajectory in another frame
    #[allow(clippy::map_clone)]
    pub fn to_frame(&self, new_frame: Frame, cosm: Arc<Cosm>) -> Result<Self, NyxError> {
        if self.states.is_empty() {
            return Err(NyxError::Trajectory(TrajError::CreationError(
                "No trajectory to convert".to_string(),
            )));
        }
        let start_instant = Instant::now();
        let mut traj = Self::new();
        // For each state, add the state and the one in between two successive states
        for these_states in self.states.windows(2) {
            let cur_sc = these_states[0];
            let state_1 = cosm.frame_chg(&cur_sc.orbit, new_frame);
            traj.states.push(cur_sc.with_orbit(state_1));
            let next_epoch = state_1.epoch() + (these_states[1].epoch() - state_1.epoch()) * 0.5;
            match self.at(next_epoch) {
                Ok(intermediate) => {
                    // The at() call will fail on the last state.
                    let state_2 = cosm.frame_chg(&intermediate.orbit, new_frame);
                    traj.states.push(cur_sc.with_orbit(state_2));
                }
                Err(e) => error!("{e} @ {next_epoch}"),
            }
        }
        // Add the final state
        let state_2 = cosm.frame_chg(&self.last().orbit, new_frame);
        traj.states.push(self.last().with_orbit(state_2));
        traj.finalize();

        info!(
            "Converted trajectory from {} to {} in {} ms: {traj}",
            self.first().orbit.frame,
            new_frame,
            (Instant::now() - start_instant).as_millis()
        );
        Ok(traj)
    }

    /// Exports this trajectory to the provided filename in CSV format with the default headers and the provided step
    pub fn to_csv_with_step(
        &self,
        filename: &str,
        step: Duration,
        cosm: Arc<Cosm>,
    ) -> Result<(), NyxError> {
        let fmtr = StateFormatter::default(filename.to_string(), cosm);
        let mut out = OrbitStateOutput::new(fmtr)?;
        for state in self.every(step) {
            out.handle(&state);
        }
        Ok(())
    }

    /// Exports this trajectory to the provided filename in CSV format with the default headers and the provided step
    pub fn to_csv_between_with_step(
        &self,
        filename: &str,
        start: Option<Epoch>,
        end: Option<Epoch>,
        step: Duration,
        cosm: Arc<Cosm>,
    ) -> Result<(), NyxError> {
        let fmtr = StateFormatter::default(filename.to_string(), cosm);
        let mut out = OrbitStateOutput::new(fmtr)?;
        let start = match start {
            Some(s) => s,
            None => self.first().epoch(),
        };
        let end = match end {
            Some(e) => e,
            None => self.last().epoch(),
        };
        for state in self.every_between(step, start, end) {
            out.handle(&state);
        }
        Ok(())
    }

    /// Exports this trajectory to the provided filename in CSV format with the default headers, one state per minute
    #[allow(clippy::identity_op)]
    pub fn to_csv(&self, filename: &str, cosm: Arc<Cosm>) -> Result<(), NyxError> {
        self.to_csv_with_step(filename, 1 * Unit::Minute, cosm)
    }

    /// Exports this trajectory to the provided filename in CSV format with the default headers, one state per minute
    #[allow(clippy::identity_op)]
    pub fn to_csv_between(
        &self,
        filename: &str,
        start: Option<Epoch>,
        end: Option<Epoch>,
        cosm: Arc<Cosm>,
    ) -> Result<(), NyxError> {
        self.to_csv_between_with_step(filename, start, end, 1 * Unit::Minute, cosm)
    }

    /// Exports this trajectory to the provided filename in CSV format with only the epoch, the geodetic latitude, longitude, and height at one state per minute.
    /// Must provide a body fixed frame to correctly compute the latitude and longitude.
    #[allow(clippy::identity_op)]
    pub fn to_groundtrack_csv(
        &self,
        filename: &str,
        body_fixed_frame: Frame,
        cosm: Arc<Cosm>,
    ) -> Result<(), NyxError> {
        let fmtr = StateFormatter::from_headers(
            vec![
                "epoch",
                "geodetic_latitude",
                "geodetic_longitude",
                "geodetic_height",
            ],
            filename.to_string(),
            cosm.clone(),
        )?;
        let mut out = OrbitStateOutput::new(fmtr)?;
        for state in self
            .to_frame(body_fixed_frame, cosm)?
            .every(1 * Unit::Minute)
        {
            out.handle(&state);
        }
        Ok(())
    }

    /// Exports this trajectory to the provided filename in CSV format with the provided headers and the provided step
    pub fn to_csv_custom(
        &self,
        filename: &str,
        headers: Vec<&str>,
        step: Duration,
        cosm: Arc<Cosm>,
    ) -> Result<(), NyxError> {
        let fmtr = StateFormatter::from_headers(headers, filename.to_string(), cosm)?;
        let mut out = OrbitStateOutput::new(fmtr)?;
        for state in self.every(step) {
            out.handle(&state);
        }
        Ok(())
    }
}

impl<S: InterpState> fmt::Display for Traj<S>
where
    DefaultAllocator:
        Allocator<f64, S::VecLength> + Allocator<f64, S::Size> + Allocator<f64, S::Size, S::Size>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dur = self.last().epoch() - self.first().epoch();
        write!(
            f,
            "Trajectory from {} to {} ({}, or {:.3} s) [{} states]",
            self.first().epoch(),
            self.last().epoch(),
            dur,
            dur.to_seconds(),
            self.states.len()
        )
    }
}
