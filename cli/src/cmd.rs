use std::fmt::Write as _;

use anyhow::{Context, anyhow, bail};
use doer_core::store::{ProjectFile, Store};
use doer_core::todo::ymd_of;
use doer_core::{Link, Priority, Project, ProjectId, Todo, TodoId};
use serde::Serialize;

use crate::store::FsStore;

pub const USAGE: &str = "\
Subcommands (for scripts and agents; the TUI live-reloads):
  doer add <text> [fields]        Add a todo, prints its id
  doer list [--project P] [--done] [--json]
  doer show <id> [--json]
  doer update <id> [fields]       Change fields; --link and --context append
  doer done <id> | undone <id> | rm <id>
  doer projects [--json]

Fields:
  --text T            --priority now|soon|someday     --due YYYY-MM-DD
  --remind-at T       --next STEP                      --session CMD
  --link 'label|url'  (repeatable, appends)
  --context TEXT      (repeatable, appends)            --notes TEXT
  --project NAME      (created if missing; 'none' for ungrouped)
  --clear FIELD       (priority, due, remind-at, next, links, session, context, notes)

<id> is any unique prefix of a todo id.";

#[derive(Default)]
struct Fields {
    text: Option<String>,
    priority: Option<String>,
    due: Option<String>,
    remind_at: Option<String>,
    next: Option<String>,
    session: Option<String>,
    notes: Option<String>,
    links: Vec<Link>,
    context: Vec<String>,
    project: Option<String>,
    clear: Vec<String>,
    json: bool,
    done: bool,
    positional: Vec<String>,
}

fn parse(args: &[String]) -> anyhow::Result<Fields> {
    let mut f = Fields::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| anyhow!("{name} needs a value"))
        };
        match arg.as_str() {
            "--text" => f.text = Some(value(arg)?),
            "--priority" => {
                let raw = value(arg)?;
                let p = Priority::parse(&raw)
                    .ok_or_else(|| anyhow!("priority must be now, soon or someday"))?;
                f.priority = Some(p.as_str().to_string());
            }
            "--due" => {
                let raw = value(arg)?;
                if doer_core::todo::parse_ymd(&raw).is_none() {
                    bail!("due must be YYYY-MM-DD");
                }
                f.due = Some(raw);
            }
            "--remind-at" => f.remind_at = Some(value(arg)?),
            "--next" => f.next = Some(value(arg)?),
            "--session" => f.session = Some(value(arg)?),
            "--notes" => f.notes = Some(value(arg)?),
            "--link" => f.links.push(parse_link(&value(arg)?)),
            "--context" => f.context.push(value(arg)?),
            "--project" => f.project = Some(value(arg)?),
            "--clear" => f.clear.push(value(arg)?),
            "--json" => f.json = true,
            "--done" => f.done = true,
            other if other.starts_with("--") => bail!("unknown option {other}"),
            _ => f.positional.push(arg.clone()),
        }
    }
    Ok(f)
}

fn parse_link(raw: &str) -> Link {
    match raw.split_once('|') {
        Some((label, url)) => Link {
            label: label.trim().to_string(),
            url: url.trim().to_string(),
        },
        None => Link {
            label: String::new(),
            url: raw.trim().to_string(),
        },
    }
}

#[must_use]
pub fn run(name: &str, args: &[String], now: i64) -> Option<anyhow::Result<()>> {
    let result = match name {
        "add" => with_store(|s| add(s, args, now)),
        "list" | "ls" => with_store(|s| list(s, args, now)),
        "show" => with_store(|s| show(s, args, now)),
        "update" | "edit" => with_store(|s| update(s, args, now)),
        "done" => with_store(|s| set_done(s, args, true, now)),
        "undone" => with_store(|s| set_done(s, args, false, now)),
        "rm" | "delete" => with_store(|s| remove(s, args)),
        "projects" => with_store(|s| projects(s, args)),
        _ => return None,
    };
    Some(result)
}

fn with_store(f: impl FnOnce(&mut Session) -> anyhow::Result<()>) -> anyhow::Result<()> {
    let store = FsStore::new()?;
    let loaded = store.load();
    let mut session = Session {
        all: loaded.value.all_todos,
        projects: loaded.value.projects,
        store,
        dirty_all: false,
        dirty: Vec::new(),
    };
    f(&mut session)?;
    session.save()
}

struct Session {
    store: FsStore,
    all: Vec<Todo>,
    projects: Vec<ProjectFile>,
    dirty_all: bool,
    dirty: Vec<ProjectId>,
}

type Home = Option<ProjectId>;

impl Session {
    fn save(&self) -> anyhow::Result<()> {
        if self.dirty_all {
            self.store.save_all_todos(&self.all)?;
        }
        for id in &self.dirty {
            if let Some(file) = self.projects.iter().find(|p| &p.id == id) {
                self.store.save_project(file)?;
            }
        }
        Ok(())
    }

    fn touch(&mut self, home: &Home) {
        match home {
            None => self.dirty_all = true,
            Some(id) if !self.dirty.contains(id) => self.dirty.push(id.clone()),
            Some(_) => {}
        }
    }

    fn todos_mut(&mut self, home: &Home) -> Option<&mut Vec<Todo>> {
        match home {
            None => Some(&mut self.all),
            Some(id) => self
                .projects
                .iter_mut()
                .find(|p| &p.id == id)
                .map(|p| &mut p.todos),
        }
    }

    fn every(&self) -> impl Iterator<Item = (Home, &Todo)> {
        self.all.iter().map(|t| (None, t)).chain(
            self.projects
                .iter()
                .flat_map(|p| p.todos.iter().map(move |t| (Some(p.id.clone()), t))),
        )
    }

    fn find(&self, prefix: &str) -> anyhow::Result<(Home, TodoId)> {
        let matches: Vec<(Home, TodoId)> = self
            .every()
            .filter(|(_, t)| t.id.as_str().starts_with(prefix))
            .map(|(h, t)| (h, t.id.clone()))
            .collect();
        match matches.as_slice() {
            [one] => Ok(one.clone()),
            [] => bail!("no todo with id {prefix}"),
            _ => bail!("{} todos match {prefix}; use more of the id", matches.len()),
        }
    }

    fn get_mut(&mut self, home: &Home, id: &TodoId) -> anyhow::Result<&mut Todo> {
        self.todos_mut(home)
            .and_then(|todos| todos.iter_mut().find(|t| &t.id == id))
            .ok_or_else(|| anyhow!("todo {id} vanished"))
    }

    fn project_name(&self, home: &Home) -> Option<String> {
        let id = home.as_ref()?;
        self.projects
            .iter()
            .find(|p| &p.id == id)
            .map(|p| p.name.clone())
    }

    fn resolve_project(&mut self, name: &str, create: bool) -> anyhow::Result<Home> {
        if name.eq_ignore_ascii_case("none") || name.is_empty() {
            return Ok(None);
        }
        let hits: Vec<ProjectId> = self
            .projects
            .iter()
            .filter(|p| p.name.eq_ignore_ascii_case(name) || p.id.as_str() == name)
            .map(|p| p.id.clone())
            .collect();
        match hits.as_slice() {
            [one] => Ok(Some(one.clone())),
            [] if create => {
                let index = self
                    .projects
                    .iter()
                    .filter(|p| p.parent_id.is_none())
                    .map(|p| p.index + 1)
                    .max()
                    .unwrap_or(0);
                let project = Project::new(name, index, None);
                let id = project.id.clone();
                self.projects.push(ProjectFile::new(&project, Vec::new()));
                self.touch(&Some(id.clone()));
                Ok(Some(id))
            }
            [] => bail!("no project named {name}"),
            _ => bail!("more than one project is named {name}"),
        }
    }
}

fn apply(todo: &mut Todo, f: &Fields) -> anyhow::Result<()> {
    for field in &f.clear {
        let m = &mut todo.meta;
        match field.as_str() {
            "priority" => m.priority = None,
            "due" => m.due = None,
            "remind-at" | "remind_at" => m.remind_at = None,
            "next" | "next-step" | "next_step" => m.next_step = None,
            "links" => m.links.clear(),
            "session" => m.session = None,
            "context" => m.context.clear(),
            "notes" => m.notes = None,
            other => bail!("cannot clear {other}"),
        }
    }
    if let Some(text) = &f.text {
        todo.text = one_line(text);
    }
    let m = &mut todo.meta;
    set(&mut m.priority, f.priority.as_ref());
    set(&mut m.due, f.due.as_ref());
    set(&mut m.remind_at, f.remind_at.as_ref());
    set(&mut m.next_step, f.next.as_ref());
    set(&mut m.session, f.session.as_ref());
    set(&mut m.notes, f.notes.as_ref());
    m.links.extend(f.links.iter().cloned());
    m.context.extend(f.context.iter().cloned());
    Ok(())
}

fn set(slot: &mut Option<String>, value: Option<&String>) {
    if let Some(v) = value {
        *slot = Some(v.clone());
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn add(s: &mut Session, args: &[String], now: i64) -> anyhow::Result<()> {
    let f = parse(args)?;
    let text = f
        .text
        .clone()
        .or_else(|| (!f.positional.is_empty()).then(|| f.positional.join(" ")))
        .context("add needs the todo text")?;
    let home = match &f.project {
        Some(name) => s.resolve_project(name, true)?,
        None => None,
    };
    let mut todo = Todo::new(one_line(&text), now);
    apply(&mut todo, &f)?;
    let id = todo.id.clone();
    s.todos_mut(&home)
        .context("project vanished")?
        .insert(0, todo.clone());
    s.touch(&home);
    if f.json {
        print_json(&view(s, &home, &todo))?;
    } else {
        println!("{id}");
    }
    Ok(())
}

fn update(s: &mut Session, args: &[String], now: i64) -> anyhow::Result<()> {
    let f = parse(args)?;
    let prefix = f.positional.first().context("update needs a todo id")?;
    let (mut home, id) = s.find(prefix)?;
    apply(s.get_mut(&home, &id)?, &f)?;
    s.touch(&home);
    if let Some(name) = &f.project {
        let to = s.resolve_project(name, true)?;
        if to != home {
            let todos = s.todos_mut(&home).context("project vanished")?;
            let at = todos
                .iter()
                .position(|t| t.id == id)
                .context("todo vanished")?;
            let todo = todos.remove(at);
            s.todos_mut(&to)
                .context("project vanished")?
                .insert(0, todo);
            s.touch(&to);
            home = to;
        }
    }
    let todo = s.get_mut(&home, &id)?.clone();
    if f.json {
        print_json(&view(s, &home, &todo))?;
    } else {
        print!("{}", render(s, &home, &todo, now));
    }
    Ok(())
}

fn set_done(s: &mut Session, args: &[String], done: bool, now: i64) -> anyhow::Result<()> {
    let f = parse(args)?;
    if f.positional.is_empty() {
        bail!("needs a todo id");
    }
    for prefix in &f.positional {
        let (home, id) = s.find(prefix)?;
        let todo = s.get_mut(&home, &id)?;
        if todo.done != done {
            todo.toggle(now);
        }
        let text = todo.text.clone();
        s.touch(&home);
        println!("{} {text}", if done { "✓" } else { "○" });
    }
    Ok(())
}

fn remove(s: &mut Session, args: &[String]) -> anyhow::Result<()> {
    let f = parse(args)?;
    if f.positional.is_empty() {
        bail!("rm needs a todo id");
    }
    for prefix in &f.positional {
        let (home, id) = s.find(prefix)?;
        let todos = s.todos_mut(&home).context("project vanished")?;
        todos.retain(|t| t.id != id);
        s.touch(&home);
        println!("removed {id}");
    }
    Ok(())
}

fn show(s: &mut Session, args: &[String], now: i64) -> anyhow::Result<()> {
    let f = parse(args)?;
    let prefix = f.positional.first().context("show needs a todo id")?;
    let (home, id) = s.find(prefix)?;
    let todo = s.get_mut(&home, &id)?.clone();
    if f.json {
        print_json(&view(s, &home, &todo))
    } else {
        print!("{}", render(s, &home, &todo, now));
        Ok(())
    }
}

fn list(s: &mut Session, args: &[String], now: i64) -> anyhow::Result<()> {
    let f = parse(args)?;
    let only: Option<Home> = match &f.project {
        Some(name) => Some(s.resolve_project(name, false)?),
        None => None,
    };
    let rows: Vec<TodoView> = s
        .every()
        .filter(|(h, t)| (f.done || !t.done) && only.as_ref().is_none_or(|o| o == h))
        .map(|(h, t)| view(s, &h, t))
        .collect();
    if f.json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!("nothing open");
    }
    let today = now.div_euclid(86_400);
    for row in rows {
        let mut line = format!(
            "{}  {} {}",
            row.todo
                .id
                .as_str()
                .get(..8)
                .unwrap_or(row.todo.id.as_str()),
            if row.todo.done { "[x]" } else { "[ ]" },
            row.todo.text
        );
        if let Some(p) = row.todo.meta.priority() {
            let _ = write!(line, "  !{}", p.as_str());
        }
        if let Some(due) = &row.todo.meta.due {
            let _ = write!(line, "  due {}", relative_due(due, today));
        }
        if let Some(project) = &row.project {
            let _ = write!(line, "  #{project}");
        }
        println!("{line}");
    }
    Ok(())
}

#[derive(Serialize)]
struct Row<'a> {
    id: &'a ProjectId,
    name: &'a str,
    parent_id: &'a Option<ProjectId>,
    open: usize,
}

fn projects(s: &mut Session, args: &[String]) -> anyhow::Result<()> {
    let f = parse(args)?;
    let rows: Vec<Row> = s
        .projects
        .iter()
        .map(|p| Row {
            id: &p.id,
            name: &p.name,
            parent_id: &p.parent_id,
            open: p.todos.iter().filter(|t| !t.done).count(),
        })
        .collect();
    if f.json {
        return print_json(&rows);
    }
    for row in rows {
        println!("{}  {} ({} open)", row.id, row.name, row.open);
    }
    Ok(())
}

#[derive(Serialize)]
struct TodoView {
    project: Option<String>,
    project_id: Option<ProjectId>,
    #[serde(flatten)]
    todo: Todo,
}

fn view(s: &Session, home: &Home, todo: &Todo) -> TodoView {
    TodoView {
        project: s.project_name(home),
        project_id: home.clone(),
        todo: todo.clone(),
    }
}

fn print_json<T: Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

#[must_use]
pub fn relative_due(due: &str, today: i64) -> String {
    let Some(day) = doer_core::todo::parse_ymd(due) else {
        return due.to_string();
    };
    match day - today {
        0 => "today".into(),
        1 => "tomorrow".into(),
        -1 => "yesterday (overdue)".into(),
        d if d < 0 => format!("{due} (overdue)"),
        _ => {
            let (_, m, d) = ymd_of(day);
            format!("{} {d}", month(m))
        }
    }
}

fn month(m: i64) -> &'static str {
    const NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    usize::try_from(m - 1)
        .ok()
        .and_then(|i| NAMES.get(i))
        .copied()
        .unwrap_or("?")
}

fn render(s: &Session, home: &Home, todo: &Todo, now: i64) -> String {
    let m = &todo.meta;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} {}",
        if todo.done { "[x]" } else { "[ ]" },
        todo.text
    );
    let _ = writeln!(out, "id        {}", todo.id);
    if let Some(project) = s.project_name(home) {
        let _ = writeln!(out, "project   {project}");
    }
    if let Some(p) = m.priority() {
        let _ = writeln!(out, "priority  {}", p.as_str());
    }
    if let Some(due) = &m.due {
        let _ = writeln!(
            out,
            "due       {due} ({})",
            relative_due(due, now.div_euclid(86_400))
        );
    }
    if let Some(r) = &m.remind_at {
        let _ = writeln!(out, "remind    {r}");
    }
    if let Some(n) = &m.next_step {
        let _ = writeln!(out, "next      {n}");
    }
    for link in &m.links {
        let _ = writeln!(out, "link      {} {}", link.label, link.url);
    }
    if let Some(sess) = &m.session {
        let _ = writeln!(out, "session   {sess}");
    }
    for c in &m.context {
        let _ = writeln!(out, "context   • {c}");
    }
    if let Some(n) = &m.notes {
        let _ = writeln!(out, "notes     {n}");
    }
    out
}
