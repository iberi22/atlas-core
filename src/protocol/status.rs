use serde::{Deserialize, Serialize};

#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SwalStatusCode {
    // 100-199: Task Lifecycle
    TskBacklog = 100,
    TskReady = 101,
    TskDispatched = 102,
    TskRunning = 103,
    TskPaused = 104,
    TskBlockedDep = 110,
    TskBlockedAuth = 111,
    TskInReview = 120,
    TskCompleted = 130,
    TskFailed = 140,
    TskAborted = 141,

    // 200-299: Git & VFS
    GitIslandLock = 200,
    GitBranchInit = 201,
    GitTreeDirty = 202,
    GitCommitted = 203,
    GitPushed = 204,
    GitPrOpened = 210,
    GitPrReview = 211,
    GitMerged = 212,
    GitConflict = 220,
    GitIslandBreach = 221,

    // 300-399: CI / CD & Verification
    CiQueued = 300,
    CiBuilding = 301,
    CiBuildOk = 302,
    CiTestsPass = 303,
    CiDeployedStg = 304,
    CiDeployedPrd = 305,
    CiBuildFail = 310,
    CiTestFail = 311,
    CiE2eFail = 312,
    CiCovDrop = 313,

    // 400-499: Infra & Mesh
    NodOnline = 400,
    NodDegraded = 401,
    NodStandby = 402,
    NetMeshSynced = 410,
    NetOffline = 411,
    MemXavierReady = 420,
    MemSyncPending = 421,

    // 500-599: Incidents
    IncSev0Critical = 500,
    IncSev1Blocker = 501,
    IncSev2Degraded = 502,
    SecLeakDetected = 510,
    OomCrash = 520,

    // 800-899: Feature Alignment
    FeatDraft = 801,
    FeatBeta = 802,
    FeatPromoted = 803,

    // 900-999: Snippets & Archetypes
    SnpFlutterBgService = 901,
    SnpAtlasWsReconnect = 902,
    SnpRustCdylibCabi = 903,
    SnpIsarTransaction = 904,
    SnpE2ePlaywrightPwa = 905,
}

impl SwalStatusCode {
    pub fn code(&self) -> i64 {
        *self as i64
    }

    pub fn slug(&self) -> &'static str {
        match self {
            Self::TskBacklog => "TSK_BACKLOG",
            Self::TskReady => "TSK_READY",
            Self::TskDispatched => "TSK_DISPATCHED",
            Self::TskRunning => "TSK_RUNNING",
            Self::TskPaused => "TSK_PAUSED",
            Self::TskBlockedDep => "TSK_BLOCKED_DEP",
            Self::TskBlockedAuth => "TSK_BLOCKED_AUTH",
            Self::TskInReview => "TSK_IN_REVIEW",
            Self::TskCompleted => "TSK_COMPLETED",
            Self::TskFailed => "TSK_FAILED",
            Self::TskAborted => "TSK_ABORTED",

            Self::GitIslandLock => "GIT_ISLAND_LOCK",
            Self::GitBranchInit => "GIT_BRANCH_INIT",
            Self::GitTreeDirty => "GIT_TREE_DIRTY",
            Self::GitCommitted => "GIT_COMMITTED",
            Self::GitPushed => "GIT_PUSHED",
            Self::GitPrOpened => "GIT_PR_OPENED",
            Self::GitPrReview => "GIT_PR_REVIEW",
            Self::GitMerged => "GIT_MERGED",
            Self::GitConflict => "GIT_CONFLICT",
            Self::GitIslandBreach => "GIT_ISLAND_BREACH",

            Self::CiQueued => "CI_QUEUED",
            Self::CiBuilding => "CI_BUILDING",
            Self::CiBuildOk => "CI_BUILD_OK",
            Self::CiTestsPass => "CI_TESTS_PASS",
            Self::CiDeployedStg => "CI_DEPLOYED_STG",
            Self::CiDeployedPrd => "CI_DEPLOYED_PRD",
            Self::CiBuildFail => "CI_BUILD_FAIL",
            Self::CiTestFail => "CI_TEST_FAIL",
            Self::CiE2eFail => "CI_E2E_FAIL",
            Self::CiCovDrop => "CI_COV_DROP",

            Self::NodOnline => "NOD_ONLINE",
            Self::NodDegraded => "NOD_DEGRADED",
            Self::NodStandby => "NOD_STANDBY",
            Self::NetMeshSynced => "NET_MESH_SYNCED",
            Self::NetOffline => "NET_OFFLINE",
            Self::MemXavierReady => "MEM_XAVIER_READY",
            Self::MemSyncPending => "MEM_SYNC_PENDING",

            Self::IncSev0Critical => "INC_SEV0_CRITICAL",
            Self::IncSev1Blocker => "INC_SEV1_BLOCKER",
            Self::IncSev2Degraded => "INC_SEV2_DEGRADED",
            Self::SecLeakDetected => "SEC_LEAK_DETECTED",
            Self::OomCrash => "OOM_CRASH",

            Self::FeatDraft => "FEAT_DRAFT",
            Self::FeatBeta => "FEAT_BETA",
            Self::FeatPromoted => "FEAT_PROMOTED",

            Self::SnpFlutterBgService => "SNP_FLUTTER_BG_SERVICE",
            Self::SnpAtlasWsReconnect => "SNP_ATLAS_WS_RECONNECT",
            Self::SnpRustCdylibCabi => "SNP_RUST_CDYLIB_CABI",
            Self::SnpIsarTransaction => "SNP_ISAR_TRANSACTION",
            Self::SnpE2ePlaywrightPwa => "SNP_E2E_PLAYWRIGHT_PWA",
        }
    }

    pub fn from_code(code: i64) -> Option<Self> {
        match code {
            100 => Some(Self::TskBacklog),
            101 => Some(Self::TskReady),
            102 => Some(Self::TskDispatched),
            103 => Some(Self::TskRunning),
            104 => Some(Self::TskPaused),
            110 => Some(Self::TskBlockedDep),
            111 => Some(Self::TskBlockedAuth),
            120 => Some(Self::TskInReview),
            130 => Some(Self::TskCompleted),
            140 => Some(Self::TskFailed),
            141 => Some(Self::TskAborted),

            200 => Some(Self::GitIslandLock),
            201 => Some(Self::GitBranchInit),
            202 => Some(Self::GitTreeDirty),
            203 => Some(Self::GitCommitted),
            204 => Some(Self::GitPushed),
            210 => Some(Self::GitPrOpened),
            211 => Some(Self::GitPrReview),
            212 => Some(Self::GitMerged),
            220 => Some(Self::GitConflict),
            221 => Some(Self::GitIslandBreach),

            300 => Some(Self::CiQueued),
            301 => Some(Self::CiBuilding),
            302 => Some(Self::CiBuildOk),
            303 => Some(Self::CiTestsPass),
            304 => Some(Self::CiDeployedStg),
            305 => Some(Self::CiDeployedPrd),
            310 => Some(Self::CiBuildFail),
            311 => Some(Self::CiTestFail),
            312 => Some(Self::CiE2eFail),
            313 => Some(Self::CiCovDrop),

            400 => Some(Self::NodOnline),
            401 => Some(Self::NodDegraded),
            402 => Some(Self::NodStandby),
            410 => Some(Self::NetMeshSynced),
            411 => Some(Self::NetOffline),
            420 => Some(Self::MemXavierReady),
            421 => Some(Self::MemSyncPending),

            500 => Some(Self::IncSev0Critical),
            501 => Some(Self::IncSev1Blocker),
            502 => Some(Self::IncSev2Degraded),
            510 => Some(Self::SecLeakDetected),
            520 => Some(Self::OomCrash),

            801 => Some(Self::FeatDraft),
            802 => Some(Self::FeatBeta),
            803 => Some(Self::FeatPromoted),

            901 => Some(Self::SnpFlutterBgService),
            902 => Some(Self::SnpAtlasWsReconnect),
            903 => Some(Self::SnpRustCdylibCabi),
            904 => Some(Self::SnpIsarTransaction),
            905 => Some(Self::SnpE2ePlaywrightPwa),
            _ => None,
        }
    }

    pub fn can_transition(from: Self, to: Self) -> bool {
        match (from, to) {
            // Task lifecycle flow
            (Self::TskBacklog, Self::TskReady) => true,
            (Self::TskReady, Self::TskDispatched) => true,
            (Self::TskDispatched, Self::TskRunning) => true,
            (Self::TskRunning, Self::TskPaused) => true,
            (Self::TskPaused, Self::TskRunning) => true,
            (Self::TskRunning, Self::TskBlockedDep) => true,
            (Self::TskBlockedDep, Self::TskReady) => true,
            (Self::TskRunning, Self::TskBlockedAuth) => true,
            (Self::TskBlockedAuth, Self::TskRunning) => true,
            (Self::TskRunning, Self::TskInReview) => true,
            (Self::TskInReview, Self::TskCompleted) => true,
            (Self::TskInReview, Self::TskRunning) => true,
            (Self::TskRunning, Self::TskFailed) => true,
            (Self::TskRunning, Self::TskAborted) => true,

            // Git / CI transitions during running task
            (Self::TskRunning, Self::GitIslandLock) => true,
            (Self::GitIslandLock, Self::GitTreeDirty) => true,
            (Self::GitTreeDirty, Self::CiBuilding) => true,
            (Self::CiBuilding, Self::CiBuildOk) => true,
            (Self::CiBuilding, Self::CiBuildFail) => true,
            (Self::CiBuildFail, Self::GitTreeDirty) => true,
            (Self::CiBuildOk, Self::CiTestsPass) => true,
            (Self::CiBuildOk, Self::CiTestFail) => true,
            (Self::CiTestFail, Self::GitTreeDirty) => true,
            (Self::CiTestsPass, Self::GitCommitted) => true,
            (Self::GitCommitted, Self::GitPushed) => true,
            (Self::GitPushed, Self::TskInReview) => true,

            // Critical incidents can be raised from running
            (Self::TskRunning, Self::IncSev0Critical) => true,
            (Self::TskRunning, Self::IncSev1Blocker) => true,
            (Self::TskRunning, Self::SecLeakDetected) => true,

            _ => false,
        }
    }
}
