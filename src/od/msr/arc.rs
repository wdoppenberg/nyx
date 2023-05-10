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

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{Debug, Display};
use std::fs::File;
use std::ops::RangeBounds;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use crate::cosmic::Cosm;
use crate::io::watermark::pq_writer;
use crate::io::{ConfigError, ConfigRepr};
use crate::linalg::allocator::Allocator;
use crate::linalg::{DefaultAllocator, DimName};
use crate::md::trajectory::Interpolatable;
use crate::od::{Measurement, TrackingDeviceSim};
use crate::State;
use arrow::array::{ArrayRef, Float64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use hifitime::prelude::{Duration, Epoch, Format, Formatter};
use parquet::arrow::ArrowWriter;

/// Tracking arc contains the tracking data generated by the tracking devices defined in this structure.
/// This structure is shared between both simulated and real tracking arcs.
#[derive(Clone, Default, Debug)]
pub struct TrackingArc<Msr>
where
    Msr: Measurement,
    DefaultAllocator: Allocator<f64, Msr::MeasurementSize>,
{
    /// The YAML configuration to set up these devices
    pub device_cfg: String,
    /// A chronological list of the measurements to the devices used to generate these measurements. If the name of the device does not appear in the list of devices, then the measurement will be ignored.
    pub measurements: Vec<(String, Msr)>,
}

impl<Msr> Display for TrackingArc<Msr>
where
    Msr: Measurement,
    DefaultAllocator: Allocator<f64, Msr::MeasurementSize>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} measurements from {:?}",
            self.measurements.len(),
            self.device_names()
        )
    }
}

impl<Msr> TrackingArc<Msr>
where
    Msr: Measurement,
    DefaultAllocator: Allocator<f64, Msr::MeasurementSize>,
{
    /// Store this tracking arc to a parquet file.
    pub fn to_parquet_simple<P: AsRef<Path> + Debug>(
        &self,
        path: P,
    ) -> Result<PathBuf, Box<dyn Error>> {
        self.to_parquet(path, None, false)
    }

    /// Store this tracking arc to a parquet file, with optional metadata and a timestamp appended to the filename.
    pub fn to_parquet<P: AsRef<Path> + Debug>(
        &self,
        path: P,
        extra_metadata: Option<HashMap<String, String>>,
        timestamp: bool,
    ) -> Result<PathBuf, Box<dyn Error>> {
        // Build the schema
        let mut hdrs = vec![
            Field::new("Epoch:Gregorian UTC", DataType::Utf8, false),
            Field::new("Epoch:Gregorian TAI", DataType::Utf8, false),
            Field::new("Epoch:TAI (s)", DataType::Float64, false),
            Field::new("Tracking device", DataType::Utf8, false),
        ];

        let mut msr_fields = Msr::fields();

        hdrs.append(&mut msr_fields);

        // Build the schema
        let schema = Arc::new(Schema::new(hdrs));
        let mut record = Vec::new();

        // Build all of the records
        record.push(Arc::new(StringArray::from(
            self.measurements
                .iter()
                .map(|m| format!("{}", m.1.epoch()))
                .collect::<Vec<String>>(),
        )) as ArrayRef);

        record.push(Arc::new(StringArray::from(
            self.measurements
                .iter()
                .map(|m| format!("{:x}", m.1.epoch()))
                .collect::<Vec<String>>(),
        )) as ArrayRef);

        record.push(Arc::new(Float64Array::from(
            self.measurements
                .iter()
                .map(|m| m.1.epoch().to_tai_seconds())
                .collect::<Vec<f64>>(),
        )) as ArrayRef);

        record.push(Arc::new(StringArray::from(
            self.measurements
                .iter()
                .map(|m| m.0.clone())
                .collect::<Vec<String>>(),
        )) as ArrayRef);

        // Now comes the measurement data

        for obs_no in 0..Msr::MeasurementSize::USIZE {
            record.push(Arc::new(Float64Array::from(
                self.measurements
                    .iter()
                    .map(|m| m.1.observation()[obs_no])
                    .collect::<Vec<f64>>(),
            )) as ArrayRef);
        }

        // Serialize all of the devices and add that to the parquet file too.
        let mut metadata = HashMap::new();
        metadata.insert("devices".to_string(), self.device_cfg.clone());
        metadata.insert("Purpose".to_string(), "Tracking Arc Data".to_string());
        if let Some(add_meta) = extra_metadata {
            for (k, v) in add_meta {
                metadata.insert(k, v);
            }
        }

        let props = pq_writer(Some(metadata));

        let mut path_buf = path.as_ref().to_path_buf();

        if timestamp {
            if let Some(file_name) = path_buf.file_name() {
                if let Some(file_name_str) = file_name.to_str() {
                    if let Some(extension) = path_buf.extension() {
                        let stamp = Formatter::new(
                            Epoch::now().unwrap(),
                            Format::from_str("%Y-%m-%dT%H-%M-%S").unwrap(),
                        );
                        let new_file_name =
                            format!("{file_name_str}-{stamp}.{}", extension.to_str().unwrap());
                        path_buf.set_file_name(new_file_name);
                    }
                }
            }
        };

        let file = File::create(&path_buf)?;

        let mut writer = ArrowWriter::try_new(file, schema.clone(), props).unwrap();

        let batch = RecordBatch::try_new(schema, record)?;
        writer.write(&batch)?;
        writer.close()?;

        info!("Serialized {self} to {path:?}");

        // Return the path this was written to
        Ok(path_buf)
    }

    /// Returns the set of devices from which measurements were taken. This accounts for the availability of measurements, so if a device was not available, it will not appear in this set.
    pub fn device_names(&self) -> HashSet<&String> {
        let mut set = HashSet::new();
        self.measurements.iter().for_each(|(name, _msr)| {
            set.insert(name);
        });
        set
    }

    /// Returns the minimum duration between two subsequent measurements.
    /// This is important to correctly set up the propagator and not miss any measurement.
    pub fn min_duration_sep(&self) -> Option<Duration> {
        let mut windows = self.measurements.windows(2);
        let first_window = windows.next()?;
        let mut min_interval = first_window[1].1.epoch() - first_window[0].1.epoch();
        for window in windows {
            let interval = window[1].1.epoch() - window[0].1.epoch();
            if interval != Duration::ZERO && interval < min_interval {
                min_interval = interval;
            }
        }

        Some(min_interval)
    }

    /// If this tracking arc has devices that can be used to generate simulated measurements,
    /// then this function can be used to rebuild said measurement devices
    pub fn rebuild_devices<MsrIn, D>(
        &self,
        cosm: Arc<Cosm>,
    ) -> Result<HashMap<String, D>, ConfigError>
    where
        MsrIn: Interpolatable,
        D: TrackingDeviceSim<MsrIn, Msr>,
        DefaultAllocator: Allocator<f64, <MsrIn as State>::Size>
            + Allocator<f64, <MsrIn as State>::Size, <MsrIn as State>::Size>
            + Allocator<f64, <MsrIn as State>::VecLength>,
    {
        let devices_repr = D::IntermediateRepr::loads_many(&self.device_cfg)?;

        let mut devices = HashMap::new();

        for serde in devices_repr {
            let device = D::from_config(serde, cosm.clone())?;
            if !self.device_names().contains(&device.name()) {
                warn!(
                    "{} from arc config does not appear in measurements -- ignored",
                    device.name()
                );
                continue;
            }
            devices.insert(device.name(), device);
        }

        Ok(devices)
    }

    /// Returns a new tracking arc that only contains measurements that fall within the given epoch range.
    pub fn filter_by_epoch<R: RangeBounds<Epoch>>(&self, bound: R) -> Self {
        let mut measurements = Vec::new();
        for (name, msr) in &self.measurements {
            if bound.contains(&msr.epoch()) {
                measurements.push((name.clone(), *msr));
            }
        }

        Self {
            measurements,
            device_cfg: self.device_cfg.clone(),
        }
    }

    /// Returns a new tracking arc that only contains measurements that fall within the given offset from the first epoch
    pub fn filter_by_offset<R: RangeBounds<Duration>>(&self, bound: R) -> Self {
        if self.measurements.is_empty() {
            return Self {
                device_cfg: self.device_cfg.clone(),
                measurements: Vec::new(),
            };
        }
        let ref_epoch = self.measurements[0].1.epoch();
        let mut measurements = Vec::new();
        for (name, msr) in &self.measurements {
            if bound.contains(&(msr.epoch() - ref_epoch)) {
                measurements.push((name.clone(), *msr));
            }
        }

        Self {
            measurements,
            device_cfg: self.device_cfg.clone(),
        }
    }
}
