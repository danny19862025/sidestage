//! Windows command-line quoting (used by `ShellExecuteW` parameters).
//!
//! Follows the rules `CommandLineToArgvW` and the MSVC runtime use:
//! backslashes are literal unless they precede a double quote; `2n`
//! backslashes + `"` give `n` backslashes and toggle quoting, `2n+1`
//! backslashes + `"` give `n` backslashes and a literal quote. So inside
//! quotes, backslashes before an embedded or the closing quote must be
//! doubled. Platform-neutral so it is tested on every OS.

/// Quotes one argument so it parses back unchanged.
pub fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    // Backslashes before the closing quote must not escape it.
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

/// Joins arguments into one parameter string.
pub fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|a| quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference parser implementing Microsoft's documented argv rules
    /// (as used by the MSVC CRT for arguments after the program name).
    fn parse(cmd: &str) -> Vec<String> {
        let chars: Vec<char> = cmd.chars().collect();
        let mut args = Vec::new();
        let mut i = 0;
        loop {
            while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
            if i >= chars.len() {
                break;
            }
            let mut cur = String::new();
            let mut quoted = false;
            while i < chars.len() {
                let c = chars[i];
                if c == '\\' {
                    let start = i;
                    while i < chars.len() && chars[i] == '\\' {
                        i += 1;
                    }
                    let n = i - start;
                    if i < chars.len() && chars[i] == '"' {
                        cur.extend(std::iter::repeat_n('\\', n / 2));
                        if n % 2 == 1 {
                            cur.push('"');
                            i += 1;
                        }
                    } else {
                        cur.extend(std::iter::repeat_n('\\', n));
                    }
                    continue;
                }
                if c == '"' {
                    if quoted && i + 1 < chars.len() && chars[i + 1] == '"' {
                        cur.push('"');
                        i += 2;
                        continue;
                    }
                    quoted = !quoted;
                    i += 1;
                    continue;
                }
                if !quoted && (c == ' ' || c == '\t') {
                    break;
                }
                cur.push(c);
                i += 1;
            }
            args.push(cur);
        }
        args
    }

    #[test]
    fn trailing_backslash_does_not_swallow_next_argument() {
        let args = vec![r"C:\My Files\".to_string(), "--next".to_string()];
        let joined = join_args(&args);
        assert_eq!(joined, r#""C:\My Files\\" --next"#);
        assert_eq!(parse(&joined), args);
    }

    #[test]
    fn round_trips_awkward_arguments() {
        let cases: Vec<Vec<&str>> = vec![
            vec!["plain", "two words"],
            vec![""],
            vec![r#"say "hi""#, r"back\slash"],
            vec![r"\\server\share\", r"ends\\"],
            vec![r#"a\"b"#, r#"\""#, "tab\there"],
            vec![r"C:\path\file.txt", r"C:\dir with space\"],
        ];
        for case in cases {
            let args: Vec<String> = case.iter().map(|s| s.to_string()).collect();
            assert_eq!(
                parse(&join_args(&args)),
                args,
                "{case:?} → {}",
                join_args(&args)
            );
        }
    }

    #[test]
    fn simple_arguments_stay_unquoted() {
        assert_eq!(quote_arg(r"C:\path\file.txt"), r"C:\path\file.txt");
        assert_eq!(quote_arg("--flag=1"), "--flag=1");
    }
}
