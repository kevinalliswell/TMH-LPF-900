use tmh_core::config::CommConfig;

fn base() -> CommConfig {
    CommConfig::from_json(include_str!("../../../config/comm_config.json")).unwrap()
}

#[test]
fn unsafe_numeric_values_and_unusable_cadences_are_rejected() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1, 0.0] {
        let mut config = base();
        config.flow_scaling.h2 = value;
        assert!(config.validate().is_err(), "flow scaling {value}");
        let mut config = base();
        config.temp.scale = value;
        assert!(config.validate().is_err(), "temperature scaling {value}");
        let mut config = base();
        config.gas_safety_limits.co = value;
        assert!(config.validate().is_err(), "gas safety limit {value}");
    }
    for value in [f64::NAN, f64::INFINITY, 1e308, 1e-300, -1.0, 0.0] {
        let mut config = base();
        config.sampling.interval_s = value;
        config.performance.experiment_data_interval = value;
        assert!(config.validate().is_err(), "sampling {value}");
        let mut config = base();
        config.mfc.timeout = value;
        assert!(config.validate().is_err(), "timeout {value}");
    }
}

#[test]
fn serial_and_protocol_shape_must_match_supported_runtime() {
    let mutations: &[fn(&mut CommConfig)] = &[
        |c| c.mfc.baudrate = 0,
        |c| c.balance.bytesize = 9,
        |c| c.temp.parity = "invalid".into(),
        |c| c.mfc.stopbits = 3,
        |c| c.mfc.port = "COM4\n".into(),
        |c| c.temp.slave_address = 248,
        |c| c.slave_address_mfc.h2 = 100,
        |c| c.slave_address_mfc.h2 = 0,
        |c| c.temp.temp_channels.swap(0, 1),
        |c| c.temp.port = c.mfc.port.to_lowercase(),
        |c| c.temp.start_reg = 1,
        |c| c.temp.reg_count = 8,
        |c| {
            c.temp.temp_channels.pop();
        },
    ];
    for (i, mutate) in mutations.iter().enumerate() {
        let mut config = base();
        mutate(&mut config);
        assert!(config.validate().is_err(), "mutation {i}");
    }
    let mut legacy = base();
    legacy.temp.slave_address = 0;
    legacy.validate().unwrap();
}

#[test]
fn old_sampling_alias_cannot_silently_disagree_with_the_applied_sampling_value() {
    let mut config = base();
    config.performance.experiment_data_interval = 2.0;
    assert!(config.validate().is_err());
    config.sampling.interval_s = 2.0;
    config.validate().unwrap();
}

#[test]
fn legacy_save_cannot_publish_invalid_configuration() {
    let directory = std::env::temp_dir().join(format!("tmh_invalid_save_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("comm_config.json");
    let mut config = base();
    config.flow_scaling.h2 = f64::NAN;
    let result = config.save(&path);
    let exists = path.exists();
    std::fs::remove_dir_all(directory).unwrap();
    assert!(result.is_err());
    assert!(
        !exists,
        "failed validation must not publish an unreadable configuration"
    );
}

#[test]
fn configuration_must_preserve_the_fixed_nitrogen_protection_setpoint() {
    for scale in [1e308, 1e-300, 3.0, f64::from_bits(1)] {
        let mut config = base();
        config.flow_scaling.n2 = scale;
        assert!(
            config.validate().is_err(),
            "N2 scaling {scale} cannot encode protection 5"
        );
    }
    for scale in [0.1, 1.0, 2.5, 50.0] {
        let mut config = base();
        config.flow_scaling.n2 = scale;
        config.validate().unwrap();
    }
    let mut config = base();
    config.flow_scaling.h2 = f64::from_bits(1);
    assert!(
        config.validate().is_err(),
        "wire resolution must not underflow to zero"
    );
}

#[test]
fn field_configuration_cannot_raise_either_flammable_ceiling() {
    for gas in [0, 3] {
        let mut config = base();
        if gas == 0 {
            config.gas_safety_limits.h2 = 5.01;
        } else {
            config.gas_safety_limits.co = 5.01;
        }
        assert!(config.validate().is_err());
    }
    assert!(base().validate().is_ok());
}
