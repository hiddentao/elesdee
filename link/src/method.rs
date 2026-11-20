//! The allowlist of methods that a paired phone may call.

use std::fmt;
use std::str::FromStr;

use crate::LinkError;

/// A method that a paired phone may call.
///
/// The allowlist follows the design table. A method that is not listed here is
/// rejected with [`LinkError::UnknownMethod`], which maps to the `forbidden`
/// error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    /// Full snapshot of projects, accounts, sessions and questions.
    Bootstrap,
    /// Agent versions, read only.
    Health,
    /// List projects for the new-session form.
    ProjectsList,
    /// List branches for the new-session form.
    BranchesList,
    /// List providers for the new-session form.
    ProvidersList,
    /// List account metadata and usage. Never keys.
    AccountsList,
    /// Create a session.
    SessionsCreate,
    /// Close a session.
    SessionsClose,
    /// Send text or key bytes to a session.
    SessionsInput,
    /// Allow or deny one permission request.
    SessionsPermission,
    /// Switch a session to another account.
    SessionsAccount,
    /// Switch a session to another AI config.
    SessionsConfig,
    /// Resume a paused session.
    SessionsResume,
    /// Upload one file to a session.
    SessionsUpload,
    /// List pending questions.
    QuestionsList,
    /// Answer one question.
    QuestionsAnswer,
    /// Approve an agent-started session.
    SessionRequestsApprove,
    /// Deny an agent-started session.
    SessionRequestsDeny,
    /// List files in the session folder and worktree.
    FilesList,
    /// Read a text file.
    FilesRead,
    /// Read an image file.
    FilesImage,
    /// Read the session's changes since it branched.
    DiffRead,
    /// Subscribe to the event stream.
    EventsSubscribe,
    /// Subscribe to a session's output stream.
    OutputSubscribe,
    /// Subscribe to a session's structured conversation.
    HistorySubscribe,
    /// Return the UI package manifest.
    UiManifest,
    /// Download the UI package.
    UiDownload,
    /// List the registered demo-server ports.
    DemoServersList,
}

impl Method {
    /// Every allowed method, in the order of the design table.
    pub const ALL: [Method; 28] = [
        Self::Bootstrap,
        Self::Health,
        Self::ProjectsList,
        Self::BranchesList,
        Self::ProvidersList,
        Self::AccountsList,
        Self::SessionsCreate,
        Self::SessionsClose,
        Self::SessionsInput,
        Self::SessionsPermission,
        Self::SessionsAccount,
        Self::SessionsConfig,
        Self::SessionsResume,
        Self::SessionsUpload,
        Self::QuestionsList,
        Self::QuestionsAnswer,
        Self::SessionRequestsApprove,
        Self::SessionRequestsDeny,
        Self::FilesList,
        Self::FilesRead,
        Self::FilesImage,
        Self::DiffRead,
        Self::EventsSubscribe,
        Self::OutputSubscribe,
        Self::HistorySubscribe,
        Self::UiManifest,
        Self::UiDownload,
        Self::DemoServersList,
    ];

    /// Returns the dotted wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Health => "health",
            Self::ProjectsList => "projects.list",
            Self::BranchesList => "branches.list",
            Self::ProvidersList => "providers.list",
            Self::AccountsList => "accounts.list",
            Self::SessionsCreate => "sessions.create",
            Self::SessionsClose => "sessions.close",
            Self::SessionsInput => "sessions.input",
            Self::SessionsPermission => "sessions.permission",
            Self::SessionsAccount => "sessions.account",
            Self::SessionsConfig => "sessions.config",
            Self::SessionsResume => "sessions.resume",
            Self::SessionsUpload => "sessions.upload",
            Self::QuestionsList => "questions.list",
            Self::QuestionsAnswer => "questions.answer",
            Self::SessionRequestsApprove => "sessionRequests.approve",
            Self::SessionRequestsDeny => "sessionRequests.deny",
            Self::FilesList => "files.list",
            Self::FilesRead => "files.read",
            Self::FilesImage => "files.image",
            Self::DiffRead => "diff.read",
            Self::EventsSubscribe => "events.subscribe",
            Self::OutputSubscribe => "output.subscribe",
            Self::HistorySubscribe => "history.subscribe",
            Self::UiManifest => "ui.manifest",
            Self::UiDownload => "ui.download",
            Self::DemoServersList => "demoServers.list",
        }
    }

    /// Parses a dotted wire name.
    ///
    /// Returns `None` for a method that is not on the allowlist.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "bootstrap" => Some(Self::Bootstrap),
            "health" => Some(Self::Health),
            "projects.list" => Some(Self::ProjectsList),
            "branches.list" => Some(Self::BranchesList),
            "providers.list" => Some(Self::ProvidersList),
            "accounts.list" => Some(Self::AccountsList),
            "sessions.create" => Some(Self::SessionsCreate),
            "sessions.close" => Some(Self::SessionsClose),
            "sessions.input" => Some(Self::SessionsInput),
            "sessions.permission" => Some(Self::SessionsPermission),
            "sessions.account" => Some(Self::SessionsAccount),
            "sessions.config" => Some(Self::SessionsConfig),
            "sessions.resume" => Some(Self::SessionsResume),
            "sessions.upload" => Some(Self::SessionsUpload),
            "questions.list" => Some(Self::QuestionsList),
            "questions.answer" => Some(Self::QuestionsAnswer),
            "sessionRequests.approve" => Some(Self::SessionRequestsApprove),
            "sessionRequests.deny" => Some(Self::SessionRequestsDeny),
            "files.list" => Some(Self::FilesList),
            "files.read" => Some(Self::FilesRead),
            "files.image" => Some(Self::FilesImage),
            "diff.read" => Some(Self::DiffRead),
            "events.subscribe" => Some(Self::EventsSubscribe),
            "output.subscribe" => Some(Self::OutputSubscribe),
            "history.subscribe" => Some(Self::HistorySubscribe),
            "ui.manifest" => Some(Self::UiManifest),
            "ui.download" => Some(Self::UiDownload),
            "demoServers.list" => Some(Self::DemoServersList),
            _ => None,
        }
    }

    /// Returns true when the method uses a streaming response.
    pub const fn is_streaming(self) -> bool {
        matches!(
            self,
            Self::EventsSubscribe | Self::OutputSubscribe | Self::HistorySubscribe
        )
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Method {
    type Err = LinkError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::parse(name).ok_or_else(|| LinkError::UnknownMethod(name.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED: [&str; 28] = [
        "bootstrap",
        "health",
        "projects.list",
        "branches.list",
        "providers.list",
        "accounts.list",
        "sessions.create",
        "sessions.close",
        "sessions.input",
        "sessions.permission",
        "sessions.account",
        "sessions.config",
        "sessions.resume",
        "sessions.upload",
        "questions.list",
        "questions.answer",
        "sessionRequests.approve",
        "sessionRequests.deny",
        "files.list",
        "files.read",
        "files.image",
        "diff.read",
        "events.subscribe",
        "output.subscribe",
        "history.subscribe",
        "ui.manifest",
        "ui.download",
        "demoServers.list",
    ];

    #[test]
    fn allowlist_matches_the_design_table() {
        let actual: Vec<&str> = Method::ALL.iter().map(|method| method.as_str()).collect();
        assert_eq!(actual, EXPECTED);
    }

    #[test]
    fn every_allowed_method_parses_back() {
        for method in Method::ALL {
            assert_eq!(Method::parse(method.as_str()), Some(method));
            assert_eq!(
                method.as_str().parse::<Method>().expect("method parses"),
                method
            );
        }
    }

    #[test]
    fn unknown_method_is_rejected_as_forbidden() {
        let error = "projects.create".parse::<Method>().unwrap_err();
        assert!(matches!(error, LinkError::UnknownMethod(ref name) if name == "projects.create"));
        assert_eq!(error.code(), crate::ErrorCode::Forbidden);
        assert_eq!(Method::parse("sessions.delete"), None);
    }

    #[test]
    fn only_event_and_output_subscriptions_stream() {
        let streaming: Vec<&str> = Method::ALL
            .iter()
            .filter(|method| method.is_streaming())
            .map(|method| method.as_str())
            .collect();
        assert_eq!(
            streaming,
            ["events.subscribe", "output.subscribe", "history.subscribe"]
        );
    }

    #[test]
    fn config_switch_is_allowed_and_provider_alias_stays() {
        assert_eq!(
            Method::parse("sessions.config"),
            Some(Method::SessionsConfig)
        );
        assert_eq!(
            Method::parse("sessions.account"),
            Some(Method::SessionsAccount)
        );
        assert_eq!(Method::parse("sessions.switch"), None);
    }

    #[test]
    fn demo_servers_are_listed_but_forwarding_is_not_a_method() {
        assert_eq!(
            Method::parse("demoServers.list"),
            Some(Method::DemoServersList)
        );
        assert_eq!(Method::parse("demoServers.forward"), None);
        assert_eq!(Method::parse("demoServers.open"), None);
    }
}
