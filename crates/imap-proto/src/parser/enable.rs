/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use compact_str::ToCompactString;

use crate::{
    Command,
    protocol::{capability::Capability, enable},
    receiver::{Request, bad},
};

impl Request<Command> {
    pub fn parse_enable(self) -> trc::Result<enable::Arguments> {
        let len = self.tokens.len();
        if len > 0 {
            let mut capabilities = Vec::with_capacity(len);
            for capability in self.tokens {
                capabilities.push(
                    Capability::parse(&capability.unwrap_bytes())
                        .map_err(|v| bad(self.tag.to_compact_string(), v))?,
                );
            }
            Ok(enable::Arguments {
                tag: self.tag,
                capabilities,
            })
        } else {
            Err(self.into_error("Missing arguments."))
        }
    }
}

impl Capability {
    pub fn parse(value: &[u8]) -> super::Result<Self> {
        hashify::tiny_map_ignore_case!(value,
            "IMAP4rev2" => Self::IMAP4rev2,
            "STARTTLS" => Self::StartTLS,
            "LOGINDISABLED" => Self::LoginDisabled,
            "CONDSTORE" => Self::CondStore,
            "QRESYNC" => Self::QResync,
            "UTF8=ACCEPT" => Self::Utf8Accept,
        )
        .ok_or_else(|| {
            format!(
                "Unsupported capability '{}'.",
                String::from_utf8_lossy(value)
            )
            .into()
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        protocol::{capability::Capability, enable},
        receiver::Receiver,
    };

    #[test]
    fn parse_enable() {
        let mut receiver = Receiver::new();

        assert_eq!(
            receiver
                .parse(&mut "t2 ENABLE IMAP4rev2 CONDSTORE\r\n".as_bytes().iter())
                .unwrap()
                .parse_enable()
                .unwrap(),
            enable::Arguments {
                tag: "t2".into(),
                capabilities: vec![Capability::IMAP4rev2, Capability::CondStore],
            }
        );
    }
}
