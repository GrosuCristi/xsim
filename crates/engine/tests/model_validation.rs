//! Model-parameter validation: unusable diode, BJT, JFET and switch
//! (voltage- and current-controlled) parameters are rejected before simulation with an error naming the
//! element, the model and the parameter, while the documented special values
//! ("no effect" corners, no breakdown, absent resistances) stay legal.

use xsim_spice_engine::*;

/// `device` between nodes `n0`, `n1`, … with every terminal tied to ground
/// through a resistor, so only the device itself can make the circuit
/// invalid. A current switch's control source `VC` drives a resistor.
fn circuit_with(name: &str, device: Device) -> Circuit {
    let mut c = Circuit::new();
    if let Device::CurrentSwitch { control, .. } = &device {
        let n = c.node("ctl");
        c.add(Element::new(
            control.clone(),
            [n, GROUND],
            Device::VoltageSource(Source::dc(1.0)),
        ));
        c.add(Element::new("RC", [n, GROUND], Device::resistor(1e3)));
    }
    let nodes: Vec<_> = (0..device.terminal_count())
        .map(|i| c.node(&format!("n{i}")))
        .collect();
    for (i, &n) in nodes.iter().enumerate() {
        c.add(Element::new(
            format!("R{i}"),
            [n, GROUND],
            Device::resistor(1e3),
        ));
    }
    c.add(Element::new(name, nodes, device));
    c
}

fn error_of(name: &str, device: Device) -> Option<String> {
    let c = circuit_with(name, device);
    let validated = c.validate().err().map(|e| e.message);
    // A simulation reports the same error without building any device.
    let simulated = simulate(
        &c,
        &Analysis::OperatingPoint,
        &SimOptions::default(),
        &SimControl::new(),
    );
    match (&validated, simulated) {
        (Some(message), Err(SimError::InvalidCircuit(e))) => assert_eq!(&e.message, message),
        (None, Ok(_)) => {}
        (v, s) => panic!("validate {v:?} but simulate {s:?}"),
    }
    validated
}

/// Asserts that `device` is rejected with a message naming `element`, its
/// model and `param`.
fn assert_rejected(element: &str, device: Device, param: &str) {
    let model = match &device {
        Device::Diode { model, .. } => model.name.clone(),
        Device::Bjt { model, .. } => model.name.clone(),
        Device::Jfet { model, .. } => model.name.clone(),
        Device::VoltageSwitch { model } => model.name.clone(),
        Device::CurrentSwitch { model, .. } => model.name.clone(),
        other => panic!("not a model device: {other:?}"),
    };
    let message =
        error_of(element, device).unwrap_or_else(|| panic!("{element}: {param} accepted"));
    let expected = format!("{element}: model {model} parameter {param} must be");
    assert!(
        message.starts_with(&expected),
        "{param}: message {message:?} does not start with {expected:?}"
    );
}

fn assert_accepted(element: &str, device: Device) {
    if let Some(message) = error_of(element, device) {
        panic!("{element} rejected: {message}");
    }
}

#[test]
fn default_and_library_models_are_valid() {
    assert_accepted("D1", Device::diode(DiodeModel::default()));
    for polarity in [Polarity::N, Polarity::P] {
        assert_accepted(
            "Q1",
            Device::bjt(BjtModel {
                polarity,
                ..BjtModel::default()
            }),
        );
        assert_accepted(
            "J1",
            Device::jfet(JfetModel {
                polarity,
                ..JfetModel::default()
            }),
        );
    }
    assert_accepted(
        "S1",
        Device::VoltageSwitch {
            model: SwitchModel::default(),
        },
    );
    assert_accepted("W1", current_switch(CurrentSwitchModel::default()));
    for e in library::diodes() {
        assert_accepted("D1", Device::diode(e.model));
    }
    for e in library::bjts() {
        assert_accepted("Q1", Device::bjt(e.model));
    }
    for e in library::jfets() {
        assert_accepted("J1", Device::jfet(e.model));
    }
}

#[test]
fn the_message_names_element_model_parameter_and_value() {
    let model = DiodeModel {
        name: "DX".into(),
        n: 0.0,
        ..DiodeModel::default()
    };
    assert_eq!(
        error_of("D7", Device::diode(model)).as_deref(),
        Some("D7: model DX parameter N must be positive and finite, got 0")
    );
    let model = BjtModel {
        name: "QX".into(),
        vaf: -50.0,
        ..BjtModel::default()
    };
    assert_eq!(
        error_of("Q3", Device::bjt(model)).as_deref(),
        Some("Q3: model QX parameter VAF must be positive, 0 or infinite (no effect), got -50")
    );
}

#[test]
fn unusable_diode_parameters_are_rejected() {
    type Edit = fn(&mut DiodeModel, f64);
    let cases: &[(&str, Edit, &[f64])] = &[
        ("N", |m, v| m.n = v, &[0.0, -1.0, f64::NAN, f64::INFINITY]),
        ("IS", |m, v| m.is = v, &[-1e-14, f64::NAN, f64::INFINITY]),
        ("RS", |m, v| m.rs = v, &[-1.0, f64::NAN, f64::INFINITY]),
        ("CJO", |m, v| m.cjo = v, &[-1e-12, f64::NAN]),
        ("VJ", |m, v| m.vj = v, &[0.0, -0.7, f64::NAN]),
        ("M", |m, v| m.m = v, &[-0.1, f64::NAN]),
        ("TT", |m, v| m.tt = v, &[-1e-9, f64::INFINITY]),
        (
            "BV",
            |m, v| m.bv = v,
            &[0.0, -5.0, f64::NAN, f64::NEG_INFINITY],
        ),
        ("IBV", |m, v| m.ibv = v, &[0.0, -1e-3, f64::NAN]),
        ("NBV", |m, v| m.nbv = v, &[0.0, -2.0, f64::NAN]),
        ("EG", |m, v| m.eg = v, &[0.0, -1.1, f64::NAN]),
        ("XTI", |m, v| m.xti = v, &[-3.0, f64::NAN]),
        ("FC", |m, v| m.fc = v, &[-0.5, f64::NAN]),
        ("KF", |m, v| m.kf = v, &[f64::NAN, f64::INFINITY]),
        ("AF", |m, v| m.af = v, &[f64::NAN]),
        (
            "TNOM",
            |m, v| m.tnom = v,
            &[-300.0, f64::NAN, f64::INFINITY],
        ),
    ];
    for &(param, edit, values) in cases {
        for &v in values {
            let mut model = DiodeModel {
                name: "DTEST".into(),
                ..DiodeModel::default()
            };
            edit(&mut model, v);
            assert_rejected("D1", Device::diode(model), param);
        }
    }
}

#[test]
fn unusable_bjt_parameters_are_rejected() {
    type Edit = fn(&mut BjtModel, f64);
    let cases: &[(&str, Edit, &[f64])] = &[
        ("IS", |m, v| m.is = v, &[-1e-16, f64::NAN]),
        (
            "BF",
            |m, v| m.bf = v,
            &[0.0, -100.0, f64::NAN, f64::INFINITY],
        ),
        ("BR", |m, v| m.br = v, &[0.0, -1.0]),
        ("NF", |m, v| m.nf = v, &[0.0, -1.0, f64::NAN]),
        ("NR", |m, v| m.nr = v, &[0.0]),
        ("NE", |m, v| m.ne = v, &[0.0, -1.5]),
        ("NC", |m, v| m.nc = v, &[0.0]),
        ("ISE", |m, v| m.ise = v, &[-1e-14]),
        ("ISC", |m, v| m.isc = v, &[-1e-14]),
        (
            "VAF",
            |m, v| m.vaf = v,
            &[-50.0, f64::NAN, f64::NEG_INFINITY],
        ),
        ("VAR", |m, v| m.var = v, &[-5.0]),
        ("IKF", |m, v| m.ikf = v, &[-0.1, f64::NAN]),
        ("IKR", |m, v| m.ikr = v, &[-0.1]),
        ("RB", |m, v| m.rb = v, &[-10.0, f64::INFINITY]),
        ("IRB", |m, v| m.irb = v, &[-1e-3, f64::NAN]),
        ("RBM", |m, v| m.rbm = v, &[-1.0]),
        ("RE", |m, v| m.re = v, &[-0.5]),
        ("RC", |m, v| m.rc = v, &[-2.0, f64::NAN]),
        ("CJE", |m, v| m.cje = v, &[-1e-12]),
        ("VJE", |m, v| m.vje = v, &[0.0, -0.75]),
        ("MJE", |m, v| m.mje = v, &[-0.3]),
        ("CJC", |m, v| m.cjc = v, &[-1e-12]),
        ("VJC", |m, v| m.vjc = v, &[0.0]),
        ("MJC", |m, v| m.mjc = v, &[f64::NAN]),
        ("XCJC", |m, v| m.xcjc = v, &[-0.1, 1.5, f64::NAN]),
        ("CJS", |m, v| m.cjs = v, &[-1e-12]),
        ("VJS", |m, v| m.vjs = v, &[0.0]),
        ("MJS", |m, v| m.mjs = v, &[-0.5]),
        ("TF", |m, v| m.tf = v, &[-1e-10, f64::INFINITY]),
        ("XTF", |m, v| m.xtf = v, &[-1.0]),
        ("VTF", |m, v| m.vtf = v, &[-2.0, f64::NAN]),
        ("ITF", |m, v| m.itf = v, &[-0.1]),
        ("PTF", |m, v| m.ptf = v, &[f64::NAN, f64::INFINITY]),
        ("TR", |m, v| m.tr = v, &[-1e-8]),
        ("EG", |m, v| m.eg = v, &[0.0]),
        ("XTB", |m, v| m.xtb = v, &[-1.5]),
        ("XTI", |m, v| m.xti = v, &[-3.0]),
        ("FC", |m, v| m.fc = v, &[-0.5]),
        ("TNOM", |m, v| m.tnom = v, &[-274.0]),
    ];
    for polarity in [Polarity::N, Polarity::P] {
        for &(param, edit, values) in cases {
            for &v in values {
                let mut model = BjtModel {
                    name: "QTEST".into(),
                    polarity,
                    ..BjtModel::default()
                };
                edit(&mut model, v);
                assert_rejected("Q1", Device::bjt(model), param);
            }
        }
    }
}

#[test]
fn unusable_jfet_parameters_are_rejected() {
    type Edit = fn(&mut JfetModel, f64);
    let cases: &[(&str, Edit, &[f64])] = &[
        ("BETA", |m, v| m.beta = v, &[0.0, -1e-3, f64::NAN]),
        ("PB", |m, v| m.pb = v, &[0.0, -1.0]),
        ("VTO", |m, v| m.vto = v, &[f64::NAN, f64::NEG_INFINITY]),
        ("LAMBDA", |m, v| m.lambda = v, &[f64::NAN, f64::INFINITY]),
        ("RD", |m, v| m.rd = v, &[-1.0]),
        ("RS", |m, v| m.rs = v, &[-1.0, f64::NAN]),
        ("CGS", |m, v| m.cgs = v, &[-1e-12]),
        ("CGD", |m, v| m.cgd = v, &[-1e-12]),
        ("IS", |m, v| m.is = v, &[-1e-14]),
        ("B", |m, v| m.b = v, &[-0.5, f64::NAN]),
        ("VTOTC", |m, v| m.vtotc = v, &[f64::NAN]),
        ("BETATCE", |m, v| m.betatce = v, &[f64::INFINITY]),
        ("FC", |m, v| m.fc = v, &[-0.5]),
        ("TNOM", |m, v| m.tnom = v, &[f64::NAN]),
    ];
    for polarity in [Polarity::N, Polarity::P] {
        for &(param, edit, values) in cases {
            for &v in values {
                let mut model = JfetModel {
                    name: "JTEST".into(),
                    polarity,
                    ..JfetModel::default()
                };
                edit(&mut model, v);
                assert_rejected("J1", Device::jfet(model), param);
            }
        }
    }
}

#[test]
fn unusable_switch_parameters_are_rejected() {
    type Edit = fn(&mut SwitchModel, f64);
    let cases: &[(&str, Edit, &[f64])] = &[
        ("RON", |m, v| m.ron = v, &[0.0, -1.0, f64::NAN]),
        ("ROFF", |m, v| m.roff = v, &[0.0, f64::INFINITY]),
        ("VT", |m, v| m.vt = v, &[f64::NAN]),
        ("VH", |m, v| m.vh = v, &[f64::INFINITY]),
    ];
    for &(param, edit, values) in cases {
        for &v in values {
            let mut model = SwitchModel::default();
            edit(&mut model, v);
            assert_rejected("S1", Device::VoltageSwitch { model }, param);
        }
    }
}

/// A current switch controlled by `VC` (see [`circuit_with`]).
fn current_switch(model: CurrentSwitchModel) -> Device {
    Device::CurrentSwitch {
        control: "VC".into(),
        model,
    }
}

#[test]
fn unusable_current_switch_parameters_are_rejected() {
    type Edit = fn(&mut CurrentSwitchModel, f64);
    let cases: &[(&str, Edit, &[f64])] = &[
        ("RON", |m, v| m.ron = v, &[0.0, -1.0, f64::NAN]),
        ("ROFF", |m, v| m.roff = v, &[0.0, f64::INFINITY]),
        ("IT", |m, v| m.it = v, &[f64::NAN, f64::NEG_INFINITY]),
        ("IH", |m, v| m.ih = v, &[f64::INFINITY]),
    ];
    for &(param, edit, values) in cases {
        for &v in values {
            let mut model = CurrentSwitchModel::default();
            edit(&mut model, v);
            assert_rejected("W1", current_switch(model), param);
        }
    }
}

#[test]
fn documented_special_values_stay_legal() {
    // No breakdown, absent series resistance, junction capacitance and
    // saturation current.
    assert_accepted(
        "D1",
        Device::diode(DiodeModel {
            bv: f64::INFINITY,
            rs: 0.0,
            cjo: 0.0,
            is: 0.0,
            fc: 1.2,
            ..DiodeModel::default()
        }),
    );
    // Corners and Early voltages spelled 0 or +∞, RBM above RB (clamped).
    assert_accepted(
        "Q1",
        Device::bjt(BjtModel {
            vaf: 0.0,
            var: f64::INFINITY,
            ikf: 0.0,
            ikr: f64::INFINITY,
            irb: 0.0,
            vtf: 0.0,
            rb: 10.0,
            rbm: 20.0,
            xcjc: 0.0,
            ..BjtModel::default()
        }),
    );
    // Enhancement-mode threshold, negative drifts and channel-length
    // modulation.
    assert_accepted(
        "J1",
        Device::jfet(JfetModel {
            vto: 0.5,
            lambda: -0.01,
            vtotc: -2e-3,
            betatce: -0.5,
            b: 0.0,
            ..JfetModel::default()
        }),
    );
    // A negative hysteresis selects the smooth switch mode.
    assert_accepted(
        "S1",
        Device::VoltageSwitch {
            model: SwitchModel {
                vh: -0.1,
                vt: -1.0,
                ..SwitchModel::default()
            },
        },
    );
    assert_accepted(
        "W1",
        current_switch(CurrentSwitchModel {
            it: -1e-3,
            ih: -1e-4,
            ..CurrentSwitchModel::default()
        }),
    );
}

#[test]
fn instance_area_is_still_checked() {
    for area in [0.0, -1.0, f64::NAN] {
        let message = error_of(
            "D1",
            Device::Diode {
                model: DiodeModel::default(),
                area,
            },
        )
        .expect("bad area accepted");
        assert!(
            message.starts_with("D1: area must be positive"),
            "{message}"
        );
    }
}

// ------------------------------------------- serialization (INT-5)

/// A minimal stand-in for JSON's data model (the engine has no JSON
/// dependency): numbers, strings, `null` and objects. Like `serde_json` it
/// writes a non-finite `f64` as `null` and cannot read `null` back as an
/// `f64`, which is exactly what made the infinite model defaults fail.
mod json_like {
    use serde::de::value::{MapDeserializer, StringDeserializer};
    use serde::de::{self, IntoDeserializer, Visitor};
    use serde::ser::{self, Impossible, Serialize, SerializeStruct};
    use serde::{Deserialize, forward_to_deserialize_any};
    use std::fmt;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Null,
        Number(f64),
        String(String),
        Object(Vec<(String, Value)>),
    }

    #[derive(Debug)]
    pub struct Error(String);

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl std::error::Error for Error {}

    impl ser::Error for Error {
        fn custom<T: fmt::Display>(message: T) -> Self {
            Self(message.to_string())
        }
    }

    impl de::Error for Error {
        fn custom<T: fmt::Display>(message: T) -> Self {
            Self(message.to_string())
        }
    }

    pub fn to_value<T: Serialize>(value: &T) -> Result<Value, Error> {
        value.serialize(Writer)
    }

    pub fn from_value<'de, T: Deserialize<'de>>(value: Value) -> Result<T, Error> {
        T::deserialize(value)
    }

    struct Writer;

    fn unsupported<T>(what: &str) -> Result<T, Error> {
        Err(Error(format!("{what} is not part of the model data")))
    }

    impl ser::Serializer for Writer {
        type Ok = Value;
        type Error = Error;
        type SerializeSeq = Impossible<Value, Error>;
        type SerializeTuple = Impossible<Value, Error>;
        type SerializeTupleStruct = Impossible<Value, Error>;
        type SerializeTupleVariant = Impossible<Value, Error>;
        type SerializeMap = Impossible<Value, Error>;
        type SerializeStruct = Object;
        type SerializeStructVariant = Impossible<Value, Error>;

        fn serialize_f64(self, v: f64) -> Result<Value, Error> {
            Ok(if v.is_finite() {
                Value::Number(v)
            } else {
                Value::Null
            })
        }
        fn serialize_f32(self, v: f32) -> Result<Value, Error> {
            self.serialize_f64(f64::from(v))
        }
        fn serialize_str(self, v: &str) -> Result<Value, Error> {
            Ok(Value::String(v.to_string()))
        }
        fn serialize_unit_variant(
            self,
            _: &'static str,
            _: u32,
            variant: &'static str,
        ) -> Result<Value, Error> {
            self.serialize_str(variant)
        }
        fn serialize_struct(self, _: &'static str, len: usize) -> Result<Object, Error> {
            Ok(Object(Vec::with_capacity(len)))
        }
        fn serialize_none(self) -> Result<Value, Error> {
            Ok(Value::Null)
        }
        fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value, Error> {
            value.serialize(self)
        }
        fn serialize_bool(self, _: bool) -> Result<Value, Error> {
            unsupported("bool")
        }
        fn serialize_i8(self, _: i8) -> Result<Value, Error> {
            unsupported("integer")
        }
        fn serialize_i16(self, _: i16) -> Result<Value, Error> {
            unsupported("integer")
        }
        fn serialize_i32(self, _: i32) -> Result<Value, Error> {
            unsupported("integer")
        }
        fn serialize_i64(self, _: i64) -> Result<Value, Error> {
            unsupported("integer")
        }
        fn serialize_u8(self, _: u8) -> Result<Value, Error> {
            unsupported("integer")
        }
        fn serialize_u16(self, _: u16) -> Result<Value, Error> {
            unsupported("integer")
        }
        fn serialize_u32(self, _: u32) -> Result<Value, Error> {
            unsupported("integer")
        }
        fn serialize_u64(self, _: u64) -> Result<Value, Error> {
            unsupported("integer")
        }
        fn serialize_char(self, _: char) -> Result<Value, Error> {
            unsupported("char")
        }
        fn serialize_bytes(self, _: &[u8]) -> Result<Value, Error> {
            unsupported("bytes")
        }
        fn serialize_unit(self) -> Result<Value, Error> {
            unsupported("unit")
        }
        fn serialize_unit_struct(self, _: &'static str) -> Result<Value, Error> {
            unsupported("unit struct")
        }
        fn serialize_newtype_struct<T: Serialize + ?Sized>(
            self,
            _: &'static str,
            _: &T,
        ) -> Result<Value, Error> {
            unsupported("newtype struct")
        }
        fn serialize_newtype_variant<T: Serialize + ?Sized>(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: &T,
        ) -> Result<Value, Error> {
            unsupported("newtype variant")
        }
        fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Error> {
            unsupported("sequence")
        }
        fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Error> {
            unsupported("tuple")
        }
        fn serialize_tuple_struct(
            self,
            _: &'static str,
            _: usize,
        ) -> Result<Self::SerializeTupleStruct, Error> {
            unsupported("tuple struct")
        }
        fn serialize_tuple_variant(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: usize,
        ) -> Result<Self::SerializeTupleVariant, Error> {
            unsupported("tuple variant")
        }
        fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Error> {
            unsupported("map")
        }
        fn serialize_struct_variant(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: usize,
        ) -> Result<Self::SerializeStructVariant, Error> {
            unsupported("struct variant")
        }
    }

    pub struct Object(Vec<(String, Value)>);

    impl SerializeStruct for Object {
        type Ok = Value;
        type Error = Error;
        fn serialize_field<T: Serialize + ?Sized>(
            &mut self,
            key: &'static str,
            value: &T,
        ) -> Result<(), Error> {
            self.0.push((key.to_string(), value.serialize(Writer)?));
            Ok(())
        }
        fn end(self) -> Result<Value, Error> {
            Ok(Value::Object(self.0))
        }
    }

    impl<'de> IntoDeserializer<'de, Error> for Value {
        type Deserializer = Self;
        fn into_deserializer(self) -> Self {
            self
        }
    }

    impl<'de> de::Deserializer<'de> for Value {
        type Error = Error;

        fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            match self {
                Value::Null => visitor.visit_unit(),
                Value::Number(v) => visitor.visit_f64(v),
                Value::String(s) => visitor.visit_string(s),
                Value::Object(fields) => {
                    visitor.visit_map(MapDeserializer::new(fields.into_iter()))
                }
            }
        }

        fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            match self {
                Value::Null => visitor.visit_none(),
                other => visitor.visit_some(other),
            }
        }

        fn deserialize_enum<V: Visitor<'de>>(
            self,
            name: &'static str,
            variants: &'static [&'static str],
            visitor: V,
        ) -> Result<V::Value, Error> {
            match self {
                Value::String(s) => {
                    let s: StringDeserializer<Error> = s.into_deserializer();
                    s.deserialize_enum(name, variants, visitor)
                }
                other => other.deserialize_any(visitor),
            }
        }

        forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf unit unit_struct newtype_struct seq tuple
            tuple_struct map struct identifier ignored_any
        }
    }
}

/// Every parameter of `value` survives a round trip unchanged.
fn assert_round_trip<M>(value: &M) -> json_like::Value
where
    M: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let written = json_like::to_value(value).expect("serialize");
    let read: M = json_like::from_value(written.clone())
        .unwrap_or_else(|e| panic!("cannot read back {written:?}: {e}"));
    assert_eq!(&read, value);
    written
}

fn field(value: &json_like::Value, name: &str) -> json_like::Value {
    match value {
        json_like::Value::Object(fields) => fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("no field {name}")),
        other => panic!("not an object: {other:?}"),
    }
}

#[test]
fn every_model_default_round_trips_through_a_json_data_model() {
    use json_like::Value;
    let inf = Value::String("inf".into());
    let diode = assert_round_trip(&DiodeModel::default());
    assert_eq!(field(&diode, "bv"), inf);
    assert_eq!(field(&diode, "is"), Value::Number(1e-14));
    let bjt = assert_round_trip(&BjtModel::default());
    for name in ["vaf", "var", "ikf", "ikr", "irb", "vtf"] {
        assert_eq!(field(&bjt, name), inf, "{name}");
    }
    assert_eq!(field(&bjt, "polarity"), Value::String("N".into()));
    assert_round_trip(&JfetModel::default());
    assert_round_trip(&SwitchModel::default());
    let csw = assert_round_trip(&CurrentSwitchModel::default());
    assert_eq!(field(&csw, "name"), Value::String("CSW".into()));
    assert_eq!(field(&csw, "it"), Value::Number(0.0));
    let odd = CurrentSwitchModel {
        roff: f64::INFINITY,
        it: f64::NEG_INFINITY,
        ih: -0.0,
        ..CurrentSwitchModel::default()
    };
    let written = assert_round_trip(&odd);
    assert_eq!(field(&written, "roff"), inf);
    assert_eq!(field(&written, "it"), Value::String("-inf".into()));
    // Library models, a P-type transistor and the other non-finite values.
    for entry in library::diodes() {
        assert_round_trip(&entry.model);
    }
    for entry in library::bjts() {
        assert_round_trip(&entry.model);
    }
    for entry in library::jfets() {
        assert_round_trip(&entry.model);
    }
    let odd = DiodeModel {
        bv: f64::NEG_INFINITY,
        rs: -0.0,
        cjo: f64::MIN_POSITIVE / 4.0,
        ..DiodeModel::default()
    };
    let written = assert_round_trip(&odd);
    assert_eq!(field(&written, "bv"), Value::String("-inf".into()));
    let nan = JfetModel {
        lambda: f64::NAN,
        ..JfetModel::default()
    };
    let written = json_like::to_value(&nan).expect("serialize");
    assert_eq!(field(&written, "lambda"), Value::String("nan".into()));
    let read: JfetModel = json_like::from_value(written).expect("deserialize");
    assert!(read.lambda.is_nan());
}

#[test]
fn model_parameters_read_numbers_and_reject_other_values() {
    use json_like::Value;
    // Missing fields take their defaults; a field holding anything but a
    // number or one of the three non-finite spellings is an error.
    let partial = Value::Object(vec![("bv".into(), Value::Number(5.1))]);
    let model: DiodeModel = json_like::from_value(partial).expect("partial");
    assert_eq!(model.bv, 5.1);
    assert_eq!(model.is, DiodeModel::default().is);
    for bad in [Value::Null, Value::String("infinity".into())] {
        let value = Value::Object(vec![("bv".into(), bad.clone())]);
        assert!(
            json_like::from_value::<DiodeModel>(value).is_err(),
            "{bad:?} accepted"
        );
    }
}
