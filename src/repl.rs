use anyhow::Result;
use std::path::Path;

use crate::db::Cassette;

#[cfg(feature = "repl")]
mod repl_impl {
    pub use super::*;
    use rustyline::completion::{Completer, Pair};
    use rustyline::error::ReadlineError;
    use rustyline::{Config, Context, Editor};
    use rustyline::highlight::Highlighter;
    use rustyline::hint::Hinter;
    use rustyline::validate::Validator;
    use std::borrow::Cow;

    #[derive(rustyline::Helper)]
    pub struct CassetteHelper;

    impl Completer for CassetteHelper {
        type Candidate = Pair;

        fn complete(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
            let commands = [
                "insert", "query", "jsonpath", "search", "get", "scan",
                "update", "delete", "collections", "compact", "dump", "save", "help", "quit", "exit",
            ];

            let start = line[..pos].rfind(' ').map(|i| i + 1).unwrap_or(0);
            let prefix = &line[start..pos];

            let mut matches: Vec<Pair> = commands
                .iter()
                .filter(|cmd| cmd.starts_with(prefix))
                .map(|cmd| Pair {
                    display: cmd.to_string(),
                    replacement: cmd.to_string(),
                })
                .collect();

            matches.sort_by(|a, b| a.display.cmp(&b.display));
            Ok((start, matches))
        }
    }

    impl Validator for CassetteHelper {}

    impl Hinter for CassetteHelper {
        type Hint = String;
    }

    impl Highlighter for CassetteHelper {
        fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
            &'s self,
            prompt: &'p str,
            default: bool,
        ) -> Cow<'b, str> {
            let _ = default;
            Cow::Borrowed(prompt)
        }

        fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
            Cow::Owned(format!("\x1b[2m{}\x1b[0m", hint))
        }

        fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
            Cow::Borrowed(line)
        }

        fn highlight_char(&self, _line: &str, _pos: usize, _kind: rustyline::highlight::CmdKind) -> bool {
            false
        }
    }

    pub fn run_rustyline(path: &Path) -> Result<()> {
        println!("CassetteDB REPL — type 'help' for commands, 'quit' to exit.");

        let mut cassette = Cassette::open(path)?;

        let config = Config::builder()
            .history_ignore_space(true)
            .completion_type(rustyline::CompletionType::List)
            .build();

        let mut rl: Editor<CassetteHelper, _> = Editor::with_config(config)?;
        rl.set_helper(Some(CassetteHelper));

        let history_path = dirs::home_dir()
            .map(|h| h.join(".cassette_history"))
            .unwrap_or_else(|| Path::new(".cassette_history").to_path_buf());

        let _ = rl.load_history(&history_path);

        loop {
            let readline = rl.readline("cassette> ");
            match readline {
                Ok(line) => {
                    let input = line.trim();
                    if input.is_empty() {
                        continue;
                    }
                    rl.add_history_entry(input)?;

                    let parts: Vec<&str> = input.splitn(2, ' ').collect();
                    let cmd = parts[0].to_lowercase();
                    let rest = parts.get(1).copied().unwrap_or("").trim();

                    match cmd.as_str() {
                        "quit" | "exit" => {
                            println!("Bye.");
                            break;
                        }
                        "help" => print_help(),
                        "save" => {
                            if let Err(e) = cassette.save(path) {
                                eprintln!("save failed: {}", e);
                            } else {
                                println!("saved.");
                            }
                        }
                        "insert" => {
                            if let Err(e) = handle_insert(&mut cassette, rest) {
                                eprintln!("insert failed: {}", e);
                            } else if let Err(e) = cassette.save(path) {
                                eprintln!("save failed: {}", e);
                            }
                        }
                        "query" => {
                            if let Err(e) = handle_query(&cassette, rest) {
                                eprintln!("query failed: {}", e);
                            }
                        }
                        "jsonpath" => {
                            if let Err(e) = handle_jsonpath(&cassette, rest) {
                                eprintln!("jsonpath failed: {}", e);
                            }
                        }
                        "search" => {
                            if let Err(e) = handle_search(&cassette, rest) {
                                eprintln!("search failed: {}", e);
                            }
                        }
                        "get" => {
                            if let Err(e) = handle_get(&cassette, rest) {
                                eprintln!("get failed: {}", e);
                            }
                        }
                        "scan" => {
                            if let Err(e) = handle_scan(&cassette, rest) {
                                eprintln!("scan failed: {}", e);
                            }
                        }
                        "update" => {
                            if let Err(e) = handle_update(&mut cassette, rest) {
                                eprintln!("update failed: {}", e);
                            } else if let Err(e) = cassette.save(path) {
                                eprintln!("save failed: {}", e);
                            }
                        }
                        "delete" => {
                            if let Err(e) = handle_delete(&mut cassette, rest) {
                                eprintln!("delete failed: {}", e);
                            } else if let Err(e) = cassette.save(path) {
                                eprintln!("save failed: {}", e);
                            }
                        }
                        "collections" => {
                            let cols = cassette.collections();
                            if cols.is_empty() {
                                println!("no collections.");
                            } else {
                                for c in cols {
                                    println!("{}", c);
                                }
                            }
                        }
                        "compact" => match cassette.compact() {
                            Ok(n) => {
                                println!("compacted: removed {} documents", n);
                                if let Err(e) = cassette.save(path) {
                                    eprintln!("save failed: {}", e);
                                }
                            }
                            Err(e) => eprintln!("compact failed: {}", e),
                        },
                        "dump" => match serde_json::to_string_pretty(&cassette) {
                            Ok(s) => println!("{}", s),
                            Err(e) => eprintln!("dump failed: {}", e),
                        },
                        _ => eprintln!("unknown command: {}. type 'help' for options.", cmd),
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("CTRL-C");
                    break;
                }
                Err(ReadlineError::Eof) => {
                    println!("CTRL-D");
                    break;
                }
                Err(err) => {
                    eprintln!("Error: {:?}", err);
                    break;
                }
            }
        }

        let _ = rl.save_history(&history_path);
        Ok(())
    }
}

#[cfg(feature = "repl")]
pub fn run(path: &Path) -> Result<()> {
    repl_impl::run_rustyline(path)
}

#[cfg(not(feature = "repl"))]
pub fn run(path: &Path) -> Result<()> {
    use std::io::{self, Write};

    println!("CassetteDB REPL — type 'help' for commands, 'quit' to exit.");

    let mut cassette = Cassette::open(path)?;

    loop {
        print!("cassette> ");
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let cmd = parts[0].to_lowercase();
        let rest = parts.get(1).copied().unwrap_or("").trim();

        match cmd.as_str() {
            "quit" | "exit" => {
                println!("Bye.");
                break;
            }
            "help" => print_help(),
            "save" => {
                if let Err(e) = cassette.save(path) {
                    eprintln!("save failed: {}", e);
                } else {
                    println!("saved.");
                }
            }
            "insert" => {
                if let Err(e) = handle_insert(&mut cassette, rest) {
                    eprintln!("insert failed: {}", e);
                } else if let Err(e) = cassette.save(path) {
                    eprintln!("save failed: {}", e);
                }
            }
            "query" => {
                if let Err(e) = handle_query(&cassette, rest) {
                    eprintln!("query failed: {}", e);
                }
            }
            "jsonpath" => {
                if let Err(e) = handle_jsonpath(&cassette, rest) {
                    eprintln!("jsonpath failed: {}", e);
                }
            }
            "search" => {
                if let Err(e) = handle_search(&cassette, rest) {
                    eprintln!("search failed: {}", e);
                }
            }
            "get" => {
                if let Err(e) = handle_get(&cassette, rest) {
                    eprintln!("get failed: {}", e);
                }
            }
            "scan" => {
                if let Err(e) = handle_scan(&cassette, rest) {
                    eprintln!("scan failed: {}", e);
                }
            }
            "update" => {
                if let Err(e) = handle_update(&mut cassette, rest) {
                    eprintln!("update failed: {}", e);
                } else if let Err(e) = cassette.save(path) {
                    eprintln!("save failed: {}", e);
                }
            }
            "delete" => {
                if let Err(e) = handle_delete(&mut cassette, rest) {
                    eprintln!("delete failed: {}", e);
                } else if let Err(e) = cassette.save(path) {
                    eprintln!("save failed: {}", e);
                }
            }
            "collections" => {
                let cols = cassette.collections();
                if cols.is_empty() {
                    println!("no collections.");
                } else {
                    for c in cols {
                        println!("{}", c);
                    }
                }
            }
            "compact" => match cassette.compact() {
                Ok(n) => {
                    println!("compacted: removed {} documents", n);
                    if let Err(e) = cassette.save(path) {
                        eprintln!("save failed: {}", e);
                    }
                }
                Err(e) => eprintln!("compact failed: {}", e),
            },
            "dump" => match serde_json::to_string_pretty(&cassette) {
                Ok(s) => println!("{}", s),
                Err(e) => eprintln!("dump failed: {}", e),
            },
            _ => eprintln!("unknown command: {}. type 'help' for options.", cmd),
        }
    }

    Ok(())
}

fn print_help() {
    println!("commands:");
    println!("  insert <collection> <json>    insert a document");
    println!("  query <collection> <filter>   query with field=value / field>value / field<value");
    println!("  jsonpath <collection> <expr>  query with jsonpath");
    println!("  search <collection> <q>       full-text search");
    println!("  get <collection> <id>         get a document by id");
    println!("  scan <collection>             list all documents in a collection");
    println!("  update <collection> <id> <json>  update a document");
    println!("  delete <collection> <id>      delete a document");
    println!("  collections                   list collections");
    println!("  compact                       remove deleted docs and rebuild indexes");
    println!("  dump                          print entire cassette as json");
    println!("  save                          persist changes");
    println!("  help                          show this help");
    println!("  quit / exit                   leave the repl");
}

fn split_collection_rest(s: &str) -> Option<(&str, &str)> {
    let mut it = s.splitn(2, ' ');
    let coll = it.next()?.trim();
    let rest = it.next().unwrap_or("").trim();
    if coll.is_empty() {
        None
    } else {
        Some((coll, rest))
    }
}

fn split_three(s: &str) -> Option<(&str, &str, &str)> {
    let mut it = s.splitn(3, ' ');
    let first = it.next()?.trim();
    let second = it.next()?.trim();
    let third = it.next()?.trim();
    if first.is_empty() || second.is_empty() || third.is_empty() {
        None
    } else {
        Some((first, second, third))
    }
}

fn handle_insert(cassette: &mut Cassette, rest: &str) -> Result<()> {
    let (coll, json) = split_collection_rest(rest).ok_or_else(|| anyhow::anyhow!("usage: insert <collection> <json>"))?;
    let value = serde_json::from_str(json)?;
    let id = cassette.insert(coll, value)?;
    println!("inserted id: {}", id);
    Ok(())
}

fn handle_query(cassette: &Cassette, rest: &str) -> Result<()> {
    let (coll, filter) = split_collection_rest(rest).ok_or_else(|| anyhow::anyhow!("usage: query <collection> <filter>"))?;
    let results = cassette.query(coll, filter)?;
    if results.is_empty() {
        println!("no documents found.");
    } else {
        for doc in results {
            println!("{}", serde_json::to_string_pretty(doc)?);
        }
    }
    Ok(())
}

fn handle_jsonpath(cassette: &Cassette, rest: &str) -> Result<()> {
    let (coll, expr) = split_collection_rest(rest).ok_or_else(|| anyhow::anyhow!("usage: jsonpath <collection> <expr>"))?;
    let results = cassette.query_jsonpath(coll, expr)?;
    if results.is_empty() {
        println!("no documents found.");
    } else {
        for doc in results {
            println!("{}", serde_json::to_string_pretty(doc)?);
        }
    }
    Ok(())
}

fn handle_search(cassette: &Cassette, rest: &str) -> Result<()> {
    let (coll, q) = split_collection_rest(rest).ok_or_else(|| anyhow::anyhow!("usage: search <collection> <query>"))?;
    let results = cassette.search(coll, q)?;
    if results.is_empty() {
        println!("no documents found.");
    } else {
        println!("found {} document(s):", results.len());
        for doc in results {
            println!("{}", serde_json::to_string_pretty(doc)?);
        }
    }
    Ok(())
}

fn handle_get(cassette: &Cassette, rest: &str) -> Result<()> {
    let (coll, id) = split_collection_rest(rest).ok_or_else(|| anyhow::anyhow!("usage: get <collection> <id>"))?;
    match cassette.get(coll, id) {
        Some(doc) => println!("{}", serde_json::to_string_pretty(doc)?),
        None => println!("not found."),
    }
    Ok(())
}

fn handle_scan(cassette: &Cassette, rest: &str) -> Result<()> {
    let coll = rest.trim();
    if coll.is_empty() {
        return Err(anyhow::anyhow!("usage: scan <collection>"));
    }
    let results = cassette.scan(coll)?;
    if results.is_empty() {
        println!("no documents found.");
    } else {
        println!("found {} document(s):", results.len());
        for doc in results {
            println!("{}", serde_json::to_string_pretty(doc)?);
        }
    }
    Ok(())
}

fn handle_update(cassette: &mut Cassette, rest: &str) -> Result<()> {
    let (coll, id, json) = split_three(rest).ok_or_else(|| anyhow::anyhow!("usage: update <collection> <id> <json>"))?;
    let value = serde_json::from_str(json)?;
    if cassette.update(coll, id, value)? {
        println!("updated.");
    } else {
        println!("document not found or deleted.");
    }
    Ok(())
}

fn handle_delete(cassette: &mut Cassette, rest: &str) -> Result<()> {
    let (coll, id) = split_collection_rest(rest).ok_or_else(|| anyhow::anyhow!("usage: delete <collection> <id>"))?;
    if cassette.delete(coll, id)? {
        println!("deleted.");
    } else {
        println!("document not found.");
    }
    Ok(())
}
