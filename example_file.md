Not Formatted:
```
use dashmap::DashMap;
use std::sync::Arc;
use tokio::{
    task::JoinHandle,
    time::{Duration, Instant},
};
use tokio_postgres::IsolationLevel;

use crate::context::transaction::{GatewayTransaction, RequestTransactionInfo, TransactionNumber};
use crate::{
    context::{ConnectionContext, SessionId},
    error::{DocumentDBError, ErrorCode, Result},
    postgres::{conn_mgmt::Connection, PgDataClient},
};
```

Formatted:
```
use std::sync::Arc;

use dashmap::DashMap;
use tokio::{
    task::JoinHandle,
    time::{Duration, Instant},
};
use tokio_postgres::IsolationLevel;

use crate::{
    transaction::{GatewayTransaction, RequestTransactionInfo, TransactionNumber}
    context::{ConnectionContext, SessionId},
    error::{DocumentDBError, ErrorCode, Result},
    postgres::{conn_mgmt::Connection, PgDataClient},
};
```