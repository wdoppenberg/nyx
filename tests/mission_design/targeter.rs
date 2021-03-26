extern crate nyx_space as nyx;

use nyx::md::targeter::*;
use nyx::md::ui::*;

#[test]
fn tgt_basic_position() {
    if pretty_env_logger::try_init().is_err() {
        println!("could not init env_logger");
    }

    let cosm = Cosm::de438();
    let eme2k = cosm.frame("EME2000");

    let orig_dt = Epoch::from_gregorian_utc_at_midnight(2020, 1, 1);

    let xi_orig = Orbit::keplerian(8_000.0, 0.2, 30.0, 60.0, 60.0, 180.0, orig_dt, eme2k);

    let target_delta_t: Duration = xi_orig.period() / 2.0;

    let spacecraft = Spacecraft::from_srp_defaults(xi_orig, 100.0, 0.0);

    let dynamics = SpacecraftDynamics::new(OrbitalDynamics::two_body());
    let setup = Propagator::default(dynamics);

    // Try to increase SMA
    let xf_desired = Orbit::keplerian(
        8_100.0,
        0.2,
        30.0,
        60.0,
        60.0,
        180.0,
        orig_dt + target_delta_t,
        eme2k,
    );

    // Define the objectives
    let objectives = vec![
        Objective {
            parameter: StateParameter::X,
            desired_value: xf_desired.x,
            tolerance: 0.1,
        },
        Objective {
            parameter: StateParameter::Y,
            desired_value: xf_desired.y,
            tolerance: 0.1,
        },
        Objective {
            parameter: StateParameter::Z,
            desired_value: xf_desired.z,
            tolerance: 0.1,
        },
    ];

    let tgt = Targeter {
        prop: Arc::new(&setup),
        objectives,
        corrector: Corrector::Velocity,
        iterations: 50,
    };

    println!("{}", tgt);

    let solution = tgt
        .try_achieve_from(spacecraft, orig_dt, orig_dt + target_delta_t)
        .unwrap();

    println!("{}", solution);
}

#[test]
fn tgt_basic_sma() {
    if pretty_env_logger::try_init().is_err() {
        println!("could not init env_logger");
    }

    let cosm = Cosm::de438();
    let eme2k = cosm.frame("EME2000");

    let orig_dt = Epoch::from_gregorian_utc_at_midnight(2020, 1, 1);

    let xi_orig = Orbit::keplerian(8_000.0, 0.2, 30.0, 60.0, 60.0, 180.0, orig_dt, eme2k);

    let target_delta_t: Duration = xi_orig.period() / 2.0;

    let spacecraft = Spacecraft::from_srp_defaults(xi_orig, 100.0, 0.0);

    let dynamics = SpacecraftDynamics::new(OrbitalDynamics::two_body());
    let setup = Propagator::default(dynamics);

    // Try to increase SMA
    let xf_desired = Orbit::keplerian(
        8_100.0,
        0.2,
        30.0,
        60.0,
        60.0,
        180.0,
        orig_dt + target_delta_t,
        eme2k,
    );

    // Define the objective
    let objectives = vec![Objective::new(StateParameter::SMA, xf_desired.sma())];

    let tgt = Targeter::delta_r(Arc::new(&setup), objectives);

    println!("{}", tgt);

    let solution = tgt
        .try_achieve_from(spacecraft, orig_dt, orig_dt + target_delta_t)
        .unwrap();

    println!("{}", solution);
}

#[test]
fn tgt_basic_sma_position() {
    if pretty_env_logger::try_init().is_err() {
        println!("could not init env_logger");
    }

    let cosm = Cosm::de438();
    let eme2k = cosm.frame("EME2000");

    let orig_dt = Epoch::from_gregorian_utc_at_midnight(2020, 1, 1);

    let xi_orig = Orbit::keplerian(8_000.0, 0.2, 30.0, 60.0, 60.0, 180.0, orig_dt, eme2k);

    let target_delta_t: Duration = xi_orig.period() / 2.0;

    let spacecraft = Spacecraft::from_srp_defaults(xi_orig, 100.0, 0.0);

    let dynamics = SpacecraftDynamics::new(OrbitalDynamics::two_body());
    let setup = Propagator::default(dynamics);

    // Try to increase SMA
    let xf_desired = Orbit::keplerian(
        8_100.0,
        0.2,
        30.0,
        60.0,
        60.0,
        180.0,
        orig_dt + target_delta_t,
        eme2k,
    );

    // Define the objectives
    let objectives = vec![Objective {
        parameter: StateParameter::SMA,
        desired_value: xf_desired.sma(),
        tolerance: 0.1,
    }];

    let tgt = Targeter {
        prop: Arc::new(&setup),
        objectives,
        corrector: Corrector::Position,
        iterations: 50,
    };

    println!("{}", tgt);

    let solution = tgt
        .try_achieve_from(spacecraft, orig_dt, orig_dt + target_delta_t)
        .unwrap();

    println!("{}", solution);
}

#[test]
fn tgt_c3_ra_decl_velocity() {
    if pretty_env_logger::try_init().is_err() {
        println!("could not init env_logger");
    }

    let cosm = Cosm::de438();
    let eme2k = cosm.frame("EME2000");
    let luna = cosm.frame("luna");

    let orig_dt = Epoch::from_gregorian_utc_at_midnight(2020, 1, 1);

    let xi_orig = Orbit::keplerian(8_000.0, 0.2, 30.0, 60.0, 60.0, 180.0, orig_dt, eme2k);
    let xi_moon = cosm.frame_chg(&xi_orig, luna);

    let spacecraft = Spacecraft::from_srp_defaults(xi_moon, 100.0, 0.0);

    let dynamics = SpacecraftDynamics::new(OrbitalDynamics::two_body());
    let setup = Propagator::default(dynamics);

    // Define the objective
    let objectives = vec![
        Objective::new(StateParameter::C3, -2.0),
        Objective::new(StateParameter::RightAscension, 0.0),
        Objective::new(StateParameter::Declination, 0.0),
    ];

    let tgt = Targeter::delta_v(Arc::new(&setup), objectives);

    println!("{}", tgt);

    let solution = tgt
        .try_achieve_from(spacecraft, orig_dt, orig_dt + 4 * TimeUnit::Day)
        .unwrap();

    println!("{}", solution);
}
