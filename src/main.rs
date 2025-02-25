mod models;

use clap::Parser;
use crossterm::{
    event::{Event, EventStream, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use itertools::Itertools as _;
use models::{Branch, Mergeable, PullRequest, Repo};
use ratatui::{
    prelude::*,
    widgets::{Block, Cell, Row, Table, TableState},
};
use std::{collections::HashMap, time::Duration};
use tokio::{
    process::Command as AsyncCommand,
    sync::broadcast,
    task::JoinSet,
    time::{interval, MissedTickBehavior},
};

const TICK_INTERVAL: Duration = Duration::from_millis(50);
const AUTO_UPDATE: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Default)]
struct AppState {
    prs: Vec<PullRequest>,
    branches: HashMap<String, String>,
    error_message: Option<String>,
    done: bool,
}

struct App {
    receiver: broadcast::Receiver<AppEvent>,
    sender: broadcast::Sender<AppEvent>,
    repo: String,
    state: AppState,
    table_state: TableState,
    tasks: JoinSet<()>,
}

#[derive(Debug, Clone)]
enum AppEvent {
    FetchedPRs(Vec<PullRequest>),
    FetchedBranchCommit(String, String),
    Error(String),
}

#[derive(Parser)]
struct Cli {
    #[clap(short = 'R', long)]
    repo: Option<String>,
}

impl App {
    fn new(repo: String) -> Self
where {
        let (sender, receiver) = broadcast::channel(32);
        Self {
            sender,
            receiver,
            repo,
            state: AppState::default(),
            table_state: TableState::default(),
            tasks: JoinSet::new(),
        }
    }

    pub(crate) async fn run(
        mut self,
        terminal: &mut Terminal<impl Backend>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut refresh = interval(TICK_INTERVAL);
        refresh.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut auto_update = interval(AUTO_UPDATE);
        auto_update.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut events = EventStream::new();
        self.fetch_prs();
        self.fetch_last_commit("master");

        while !self.state.done {
            tokio::select! {
                _ = refresh.tick() => self.draw(terminal)?,
                _ = auto_update.tick() => self.fetch_prs(),
                Some(Ok(event)) = events.next() =>  self.handle_term_event(&event),
                Ok(app_event) = self.receiver.recv() => self.handle_app_event(app_event),
            }
        }

        Ok(())
    }

    fn row<'a>(branches: &HashMap<String, String>, pr: &'a PullRequest) -> Row<'a> {
        // Some(bool) the commit is found and matches or not
        // None the branch is not known
        let commit = branches
            .get(&pr.base_name)
            .map(|commit| &pr.base_commit == commit);

        let mergeable = match (&pr.mergeable, commit) {
            (Mergeable::Ok, Some(true)) => Cell::from("Up to date".bold().green()),
            (Mergeable::Ok, Some(false)) => Cell::from("Behind".bold().yellow()),
            (Mergeable::Ok, None) => Cell::from("Unsynced".bold().yellow()),
            (Mergeable::Conflict, _) => Cell::from("Conflict".bold().red()),
            _ => Cell::from("Unknown".magenta()),
        };

        let number = if pr.draft {
            Cell::from(format!("#{}", pr.number).dark_gray())
        } else {
            Cell::from(format!("#{}", pr.number).green())
        };

        Row::new(vec![
            number,
            Cell::from(pr.title.clone()),
            Cell::from(pr.branch.clone()).style(Style::default().fg(Color::Cyan)),
            mergeable,
        ])
    }

    fn draw(
        &mut self,
        terminal: &mut Terminal<impl Backend>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        terminal.draw(|f| {
            let size = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(100)])
                .split(size);

            let branches = self.state.branches.clone();

            let rows = self
                .state
                .prs
                .iter()
                .map(|pr| Self::row(&branches, pr))
                .collect::<Vec<_>>();

            let header = vec!["ID", "NAME", "BRANCH", "STATUS"]
                .into_iter()
                .map(Cell::from)
                .collect::<Row>();

            let constraints = [
                Constraint::Min(5),
                Constraint::Percentage(40),
                Constraint::Percentage(40),
                Constraint::Min(10),
            ];

            let selected_row_style = Style::default().add_modifier(Modifier::REVERSED);

            let mut block = Block::default();
            if let Some(msg) = &self.state.error_message {
                block = block.title_bottom(
                    Line::from(msg.to_owned()).style(Style::default().fg(Color::Red)),
                );
            }

            let table = Table::new(rows, constraints)
                .header(header.bold().dark_gray().underlined())
                .row_highlight_style(selected_row_style)
                .block(block);

            f.render_stateful_widget(table, chunks[0], &mut self.table_state);
        })?;

        Ok(())
    }

    fn open_externally(&mut self, selected: usize) {
        let url = self.state.prs[selected].url.to_string();

        self.tasks.spawn(async move {
            let _ = AsyncCommand::new("open").arg(url).output().await;
        });
    }

    fn rebase(&mut self, selected: usize) {
        let pr = &self.state.prs[selected];

        if pr.mergeable != Mergeable::Ok {
            return;
        }

        let sender = self.sender.clone();
        let number = pr.number.to_string();
        let repo = self.repo.clone();

        self.tasks.spawn(async move {
            let mut command = AsyncCommand::new("gh");
            command
                .arg("pr")
                .arg("update-branch")
                .arg("--rebase")
                .arg("-R")
                .arg(repo)
                .arg(number);

            match command.output().await {
                Ok(status) if !status.status.success() => {
                    let _ = sender.send(AppEvent::Error(
                        String::from_utf8_lossy(&status.stderr).into(),
                    ));
                }
                Err(e) => {
                    let _ = sender.send(AppEvent::Error(e.to_string()));
                }
                _ => {}
            }
        });
    }

    fn fetch_last_commit(&mut self, branch: &str) {
        let sender = self.sender.clone();
        let branch = branch.to_owned();
        let repo = self.repo.clone();

        self.tasks.spawn(async move {
            let commit = fetch_last_branch_commit(&repo, &branch)
                .await
                .unwrap_or_default();
            let _ = sender.send(AppEvent::FetchedBranchCommit(branch, commit));
        });
    }

    fn fetch_prs(&mut self) {
        let sender = self.sender.clone();
        let repo = self.repo.to_string();

        self.tasks.spawn(async move {
            let prs = fetch_prs(&repo).await.unwrap_or_default();
            let _ = sender.send(AppEvent::FetchedPRs(prs));
        });
    }

    fn handle_term_event(&mut self, event: &Event) {
        match event {
            Event::Key(key) => match key.code {
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    self.fetch_prs();
                }

                KeyCode::Char('q') | KeyCode::Char('Q') => {
                    self.state.done = true;
                }

                KeyCode::Up => {
                    if !self.state.prs.is_empty() {
                        let selected =
                            self.table_state
                                .selected()
                                .map_or(0, |i| if i > 0 { i - 1 } else { 0 });
                        self.table_state.select(Some(selected));
                    }
                }

                KeyCode::Down => {
                    let selected = self.table_state.selected().map_or(0, |i| {
                        if i < self.state.prs.len() - 1 {
                            i + 1
                        } else {
                            self.state.prs.len() - 1
                        }
                    });

                    self.table_state.select(Some(selected));
                }

                KeyCode::Enter => {
                    if let Some(selected) = self.table_state.selected() {
                        self.open_externally(selected);
                    }
                }

                KeyCode::Char('s') => {
                    if let Some(selected) = self.table_state.selected() {
                        self.rebase(selected);
                    }
                }

                _ => {}
            },
            _ => {}
        }
    }

    fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::FetchedPRs(prs) => {
                self.state.prs = prs.to_owned();
            }

            AppEvent::FetchedBranchCommit(branch, commit) => {
                self.state
                    .branches
                    .insert(branch.to_owned(), commit.to_owned());
            }

            AppEvent::Error(msg) => self.state.error_message = Some(msg.split('\n').join(" ")),
        }
    }
}

pub async fn fetch_prs(repo: &str) -> Result<Vec<PullRequest>, Box<dyn std::error::Error>> {
    let output = AsyncCommand::new("gh")
        .arg("pr")
        .arg("list")
        .arg("-a")
        .arg("@me")
        .arg("--json")
        .arg("number,title,mergeable,headRefName,baseRefName,baseRefOid,isDraft,url,statusCheckRollup")
        .arg("-R")
        .arg(repo)
        .output()
        .await?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh command failed: {}", err_msg).into());
    }

    let json_str = String::from_utf8(output.stdout)?;

    let prs: Vec<PullRequest> = serde_json::from_str(&json_str)?;

    Ok(prs)
}

pub async fn fetch_last_branch_commit(
    repo: &str,
    branch: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let output = AsyncCommand::new("gh")
        .arg("api")
        .arg(format!("/repos/{repo}/branches/{branch}"))
        .output()
        .await?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh command failed: {}", err_msg).into());
    }

    let json_str = String::from_utf8(output.stdout)?;

    let branch: Branch = serde_json::from_str(&json_str)?;

    Ok(branch.commit.sha)
}

pub async fn fetch_current_repo() -> Result<String, Box<dyn std::error::Error>> {
    let output = AsyncCommand::new("gh")
        .arg("repo")
        .arg("view")
        .arg("--json")
        .arg("name,owner")
        .output()
        .await?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh command failed: {}", err_msg).into());
    }

    let json_str = String::from_utf8(output.stdout)?;

    let data: Repo = serde_json::from_str(&json_str)?;

    Ok(format!("{}/{}", data.owner.login, data.name))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();

    let repo = if let Some(repo) = args.repo {
        repo
    } else {
        fetch_current_repo().await?
    };

    let mut terminal = setup_terminal()?;
    App::new(repo).run(&mut terminal).await?;
    restore_terminal(terminal)?;
    Ok(())
}

fn setup_terminal() -> Result<Terminal<impl Backend>, Box<dyn std::error::Error>> {
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;
    execute!(std::io::stdout(), EnterAlternateScreen)?; // Enter alternate screen
    terminal.hide_cursor()?;
    terminal.clear()?;
    crossterm::terminal::enable_raw_mode()?; // Enable raw mode for handling key events
    Ok(terminal)
}

fn restore_terminal(
    mut terminal: Terminal<impl Backend>,
) -> Result<(), Box<dyn std::error::Error>> {
    crossterm::terminal::disable_raw_mode()?; // Disable raw mode before exiting
    execute!(std::io::stdout(), LeaveAlternateScreen)?; // Leave alternate screen
    terminal.show_cursor()?;
    Ok(())
}
