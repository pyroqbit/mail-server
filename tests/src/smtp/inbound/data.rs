/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use common::Core;
use store::Stores;
use utils::config::Config;

use crate::{
    AssertConfig,
    smtp::{
        TempDir, TestSMTP,
        inbound::TestMessage,
        session::{TestSession, VerifyResponse, load_test_message},
    },
};
use smtp::core::Session;

const CONFIG: &str = r#"
[storage]
data = "rocksdb"
lookup = "rocksdb"
blob = "rocksdb"
fts = "rocksdb"
directory = "local"

[store."rocksdb"]
type = "rocksdb"
path = "{TMP}/queue.db"

[spam-filter]
enable = false

[directory."local"]
type = "memory"

[[directory."local".principals]]
name = "john"
description = "John Doe"
secret = "secret"
email = ["john@foobar.org", "jdoe@example.org", "john.doe@example.org"]

[[directory."local".principals]]
name = "jane"
description = "Jane Doe"
secret = "p4ssw0rd"
email = "jane@domain.net"

[[directory."local".principals]]
name = "bill"
description = "Bill Foobar"
secret = "p4ssw0rd"
email = "bill@foobar.org"

[[directory."local".principals]]
name = "mike"
description = "Mike Foobar"
secret = "p4ssw0rd"
email = "mike@test.com"

[session.rcpt]
directory = "'local'"

[session.data.limits]
messages = [{if = "remote_ip = '10.0.0.1'", then = 1},
            {else = 100}]
received-headers = 3

[session.data.add-headers]
received = [{if = "remote_ip = '10.0.0.3'", then = true},
            {else = false}]
received-spf =  [{if = "remote_ip = '10.0.0.3'", then = true},
            {else = false}]
auth-results =  [{if = "remote_ip = '10.0.0.3'", then = true},
            {else = false}]
message-id =  [{if = "remote_ip = '10.0.0.3'", then = true},
               {else = false}]
date = [{if = "remote_ip = '10.0.0.3'", then = true},
        {else = false}]
return-path =  [{if = "remote_ip = '10.0.0.3'", then = true},
            {else = false}]

[[queue.quota]]
match = "sender = 'john@doe.org'"
key = ['sender']
messages = 1

[[queue.quota]]
match = "rcpt_domain = 'foobar.org'"
key = ['rcpt_domain']
size = 450
enable = true

[[queue.quota]]
match = "rcpt = 'jane@domain.net'"
key = ['rcpt']
size = 450
enable = true

"#;

#[tokio::test]
async fn data() {
    // Enable logging
    crate::enable_logging();

    // Create temp dir for queue
    let tmp_dir = TempDir::new("smtp_data_test", true);
    let mut config = Config::new(tmp_dir.update_config(CONFIG)).unwrap();
    let stores = Stores::parse_all(&mut config, false).await;
    let core = Core::parse(&mut config, stores, Default::default()).await;
    config.assert_no_errors();

    // Test queue message builder
    let test = TestSMTP::from_core(core);
    let mut qr = test.queue_receiver;
    let mut session = Session::test(test.server.clone());
    session.data.remote_ip_str = "10.0.0.1".into();
    session.eval_session_params().await;
    session.test_builder().await;

    // Send DATA without RCPT
    session.ehlo("mx.doe.org").await;
    session.ingest(b"DATA\r\n").await.unwrap();
    session.response().assert_code("503 5.5.1");

    // Send broken message
    session
        .send_message("john@doe.org", &["bill@foobar.org"], "invalid", "550 5.7.7")
        .await;

    // Naive Loop detection
    session
        .send_message(
            "john@doe.org",
            &["bill@foobar.org"],
            "test:loop",
            "450 4.4.6",
        )
        .await;

    // No headers should be added to messages from 10.0.0.1
    session
        .send_message("john@test.org", &["mike@test.com"], "test:no_msgid", "250")
        .await;
    assert_eq!(
        qr.expect_message().await.read_message(&qr).await,
        load_test_message("no_msgid", "messages")
    );

    // Maximum one message per session is allowed for 10.0.0.1
    session.mail_from("john@doe.org", "250").await;
    session.rcpt_to("bill@foobar.org", "250").await;
    session.ingest(b"DATA\r\n").await.unwrap();
    session.response().assert_code("452 4.4.5");
    session.rset().await;

    // Headers should be added to messages from 10.0.0.3
    session.data.remote_ip_str = "10.0.0.3".into();
    session.eval_session_params().await;
    session
        .send_message("bill@doe.org", &["mike@test.com"], "test:no_msgid", "250")
        .await;
    qr.expect_message()
        .await
        .read_lines(&qr)
        .await
        .assert_contains("From: ")
        .assert_contains("To: ")
        .assert_contains("Subject: ")
        .assert_contains("Date: ")
        .assert_contains("Message-ID: ")
        .assert_contains("Return-Path: ")
        .assert_contains("Received: ")
        .assert_contains("Authentication-Results: ")
        .assert_contains("Received-SPF: ");

    // Only one message is allowed in the queue from john@doe.org
    session.data.remote_ip_str = "10.0.0.2".into();
    session.eval_session_params().await;
    session
        .send_message("john@doe.org", &["bill@foobar.org"], "test:no_dkim", "250")
        .await;
    session
        .send_message(
            "john@doe.org",
            &["bill@foobar.org"],
            "test:no_dkim",
            "452 4.3.1",
        )
        .await;

    // Release quota
    qr.clear_queue(&test.server).await;

    // Only 1500 bytes are allowed in the queue to domain foobar.org
    session
        .send_message(
            "jane@foobar.org",
            &["bill@foobar.org"],
            "test:no_dkim",
            "250",
        )
        .await;
    session
        .send_message(
            "jane@foobar.org",
            &["bill@foobar.org"],
            "test:no_dkim",
            "452 4.3.1",
        )
        .await;

    // Only 1500 bytes are allowed in the queue to recipient jane@domain.net
    session
        .send_message(
            "jane@foobar.org",
            &["jane@domain.net"],
            "test:no_dkim",
            "250",
        )
        .await;
    session
        .send_message(
            "jane@foobar.org",
            &["jane@domain.net"],
            "test:no_dkim",
            "452 4.3.1",
        )
        .await;

    // Make sure store is empty
    qr.clear_queue(&test.server).await;
    test.server
        .store()
        .assert_is_empty(test.server.blob_store().clone())
        .await;
}
