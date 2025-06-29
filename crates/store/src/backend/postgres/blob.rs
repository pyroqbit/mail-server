/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::ops::Range;

use super::{PostgresStore, into_error};

impl PostgresStore {
    pub(crate) async fn get_blob(
        &self,
        key: &[u8],
        range: Range<usize>,
    ) -> trc::Result<Option<Vec<u8>>> {
        let conn = self.conn_pool.get().await.map_err(into_error)?;
        let s = conn
            .prepare_cached("SELECT v FROM t WHERE k = $1")
            .await
            .map_err(into_error)?;
        conn.query_opt(&s, &[&key])
            .await
            .and_then(|row| {
                if let Some(row) = row {
                    Ok(Some(if range.start == 0 && range.end == usize::MAX {
                        row.try_get::<_, Vec<u8>>(0)?
                    } else {
                        let bytes = row.try_get::<_, &[u8]>(0)?;
                        bytes
                            .get(range.start..std::cmp::min(bytes.len(), range.end))
                            .unwrap_or_default()
                            .to_vec()
                    }))
                } else {
                    Ok(None)
                }
            })
            .map_err(into_error)
    }

    pub(crate) async fn put_blob(&self, key: &[u8], data: &[u8]) -> trc::Result<()> {
        let conn = self.conn_pool.get().await.map_err(into_error)?;
        let s = conn
            .prepare_cached(
                "INSERT INTO t (k, v) VALUES ($1, $2) ON CONFLICT (k) DO UPDATE SET v = EXCLUDED.v",
            )
            .await
            .map_err(into_error)?;
        conn.execute(&s, &[&key, &data])
            .await
            .map_err(into_error)
            .map(|_| ())
    }

    pub(crate) async fn delete_blob(&self, key: &[u8]) -> trc::Result<bool> {
        let conn = self.conn_pool.get().await.map_err(into_error)?;
        let s = conn
            .prepare_cached("DELETE FROM t WHERE k = $1")
            .await
            .map_err(into_error)?;
        conn.execute(&s, &[&key])
            .await
            .map_err(into_error)
            .map(|hits| hits > 0)
    }
}
