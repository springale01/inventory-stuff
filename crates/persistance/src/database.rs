use std::path::PathBuf;

use settings::database::DatabaseSettings;
use sqlx::{
    SqlitePool,
    migrate::{MigrateError, Migrator},
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

static MIGRATOR: Migrator = sqlx::migrate!("../../database/migrations");

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Database max_connections must be greater than zero")]
    InvalidMaxConnections,
    #[error("Could not create database directory {path}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Database connection failed: {0}")]
    Connection(#[from] sqlx::Error),
    #[error("Database migration failed: {0}")]
    Migration(#[from] MigrateError),
}

/// A shared SQLite pool with the inventory schema initialized.
/// Clones share the same pool; they do not create additional pools.
#[derive(Debug, Clone)]
pub struct PineappleDatabase {
    pool: SqlitePool,
}

impl PineappleDatabase {
    /// Creates missing directories and the database, then applies pending embedded
    /// migrations. Relative URLs resolve from the process working directory.
    pub async fn new(settings: &DatabaseSettings) -> Result<Self, DatabaseError> {
        if settings.max_connections == 0 {
            return Err(DatabaseError::InvalidMaxConnections);
        }

        let options = settings
            .url
            .parse::<SqliteConnectOptions>()?
            .create_if_missing(true)
            .foreign_keys(true);

        if let Some(parent) = options
            .get_filename()
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await.map_err(|source| {
                DatabaseError::CreateDirectory {
                    path: parent.to_path_buf(),
                    source,
                }
            })?;
        }

        let pool = SqlitePoolOptions::new()
            .max_connections(settings.max_connections)
            .connect_with(options)
            .await?;

        if let Err(error) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(error.into());
        }

        Ok(Self { pool })
    }

    /// Borrows the pool for queries and transactions.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Waits for active connections to return and closes the shared pool.
    /// All clones and cloned pool handles are closed too.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}
