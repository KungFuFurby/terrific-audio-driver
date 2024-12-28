// SPDX-FileCopyrightText: © 2024 Marcus Rowe <undisbeliever@gmail.com>
//
// SPDX-License-Identifier: MIT

use crate::*;

#[test]
fn mono_echo_volume() {
    let dummy_data = dummy_data();

    let s = compile_mml(
        r#"
#EchoVolume 127

A r
"#,
        &dummy_data,
    );
    assert_eq!(
        s.metadata().echo_buffer.echo_volume_l,
        127u32.try_into().unwrap()
    );
    assert_eq!(
        s.metadata().echo_buffer.echo_volume_r,
        127u32.try_into().unwrap()
    );

    assert_one_header_error_in_mml(
        r#"
#EchoVolume -1

A r
"#,
        2,
        ValueError::CannotParseUnsigned("-1".to_owned()).into(),
    );

    assert_one_header_error_in_mml(
        r#"
#EchoVolume 128

A r
"#,
        2,
        ValueError::EchoVolumeOutOfRange(128).into(),
    );
}

#[test]
fn stereo_echo_volume() {
    let dummy_data = dummy_data();

    let s = compile_mml(
        r#"
#EchoVolume 0 127

A r
"#,
        &dummy_data,
    );
    assert_eq!(
        s.metadata().echo_buffer.echo_volume_l,
        0u32.try_into().unwrap()
    );
    assert_eq!(
        s.metadata().echo_buffer.echo_volume_r,
        127u32.try_into().unwrap()
    );

    assert_one_header_error_in_mml(
        r#"
#EchoVolume 64 -1

A r
"#,
        2,
        ValueError::CannotParseUnsigned("-1".to_owned()).into(),
    );

    assert_one_header_error_in_mml(
        r#"
#EchoVolume 64 128

A r
"#,
        2,
        ValueError::EchoVolumeOutOfRange(128).into(),
    );
}

#[test]
fn echo_volume_number_of_arguments_error() {
    assert_one_header_error_in_mml(
        r#"
#EchoVolume 1 2 3

A r
"#,
        2,
        MmlLineError::InvalidNumberOfEchoVolumeArguments,
    );
}