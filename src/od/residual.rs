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

use super::EpochFormat;
use crate::hifitime::Epoch;
use crate::linalg::allocator::Allocator;
use crate::linalg::{DefaultAllocator, DimName, OVector};
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};
use std::fmt;

/// Stores an Estimate, as the result of a `time_update` or `measurement_update`.
#[derive(Debug, Clone, PartialEq)]
pub struct Residual<M>
where
    M: DimName,
    DefaultAllocator: Allocator<f64, M> + Allocator<f64, M, M>,
{
    /// Date time of this Residual
    pub dt: Epoch,
    /// The prefit residual in the units of the measurement type
    pub prefit: OVector<f64, M>,
    /// The postfit residual in the units of the measurement type
    pub postfit: OVector<f64, M>,
    /// The prefit residual ratio, i.e. `r' * (H*P*H')^-1 * r`, where `r` is the prefit residual, `H` is the sensitivity matrix, and `P` is the covariance matrix.
    pub ratio: f64,
    /// The Epoch format upon serialization
    pub epoch_fmt: EpochFormat,
}

impl<M> Residual<M>
where
    M: DimName,
    DefaultAllocator: Allocator<f64, M> + Allocator<f64, M, M>,
{
    /// An empty estimate. This is useful if wanting to store an estimate outside the scope of a filtering loop.
    pub fn zeros() -> Self {
        Self {
            dt: Epoch::from_tai_seconds(0.0),
            prefit: OVector::<f64, M>::zeros(),
            postfit: OVector::<f64, M>::zeros(),
            ratio: 0.0,
            epoch_fmt: EpochFormat::GregorianUtc,
        }
    }

    pub fn header(epoch_fmt: EpochFormat) -> Vec<String> {
        let mut hdr_v = Vec::with_capacity(2 * M::dim() + 1);
        hdr_v.push(format!("{epoch_fmt}"));
        // Serialize the prefit
        for i in 0..M::dim() {
            hdr_v.push(format!("prefit_{i}"));
        }
        // Serialize the postfit
        for i in 0..M::dim() {
            hdr_v.push(format!("postfit_{i}"));
        }
        hdr_v
    }

    pub fn default_header() -> Vec<String> {
        Self::header(EpochFormat::GregorianUtc)
    }

    pub fn new(dt: Epoch, prefit: OVector<f64, M>, postfit: OVector<f64, M>, ratio: f64) -> Self {
        Self {
            dt,
            prefit,
            postfit,
            ratio,
            epoch_fmt: EpochFormat::GregorianUtc,
        }
    }
}

impl<M> fmt::Display for Residual<M>
where
    M: DimName,
    DefaultAllocator:
        Allocator<f64, M> + Allocator<f64, M, M> + Allocator<usize, M> + Allocator<usize, M, M>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Prefit {} Postfit {}", &self.prefit, &self.postfit)
    }
}

impl<M> fmt::LowerExp for Residual<M>
where
    M: DimName,
    DefaultAllocator:
        Allocator<f64, M> + Allocator<f64, M, M> + Allocator<usize, M> + Allocator<usize, M, M>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Prefit {:e} Postfit {:e}", &self.prefit, &self.postfit)
    }
}

impl<M> Serialize for Residual<M>
where
    M: DimName,
    DefaultAllocator:
        Allocator<f64, M> + Allocator<f64, M, M> + Allocator<usize, M> + Allocator<usize, M, M>,
{
    /// Serializes the estimate
    fn serialize<O>(&self, serializer: O) -> Result<O::Ok, O::Error>
    where
        O: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(2 * M::dim() + 1))?;
        match self.epoch_fmt {
            EpochFormat::GregorianUtc => seq.serialize_element(&format!("{}", self.dt))?,
            EpochFormat::GregorianTai => seq.serialize_element(&format!("{:x}", self.dt))?,
            EpochFormat::MjdTai => seq.serialize_element(&self.dt.to_mjd_tai_days())?,
            EpochFormat::MjdTt => seq.serialize_element(&self.dt.to_mjd_tt_days())?,
            EpochFormat::MjdUtc => seq.serialize_element(&self.dt.to_mjd_utc_days())?,
            EpochFormat::JdeEt => seq.serialize_element(&self.dt.to_jde_et_days())?,
            EpochFormat::JdeTai => seq.serialize_element(&self.dt.to_jde_tai_days())?,
            EpochFormat::JdeTt => seq.serialize_element(&self.dt.to_jde_tt_days())?,
            EpochFormat::JdeUtc => seq.serialize_element(&self.dt.to_jde_utc_days())?,
            EpochFormat::TaiSecs(e) => seq.serialize_element(&(self.dt.to_tai_seconds() - e))?,
            EpochFormat::TaiDays(e) => seq.serialize_element(&(self.dt.to_tai_days() - e))?,
        }
        // Serialize the prefit
        for i in 0..M::dim() {
            seq.serialize_element(&self.prefit[(i, 0)])?;
        }
        // Serialize the postfit
        for i in 0..M::dim() {
            seq.serialize_element(&self.postfit[(i, 0)])?;
        }
        // Serialize the ratio
        seq.serialize_element(&self.ratio)?;
        seq.end()
    }
}
