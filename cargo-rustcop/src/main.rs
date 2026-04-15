fn main() {
    // When invoked as `cargo rustcop`, Cargo passes "rustcop" as the first arg.
    // Strip it so clap sees only the intended arguments.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let filtered: Vec<&std::ffi::OsString> = if args.len() > 1 && args[1] == "rustcop" {
        std::iter::once(&args[0]).chain(args.iter().skip(2)).collect()
    } else {
        args.iter().collect()
    };

    rustcop::run(filtered);
}
