// search.rs — Aggregate and serialize search results.
//
// This module ties together the three result sources (calculator, app
// search, file search) into a single ordered `Vec<DisplayItem>`, and
// provides a hand-rolled JSON serializer (`search_json`) that the
// Electron frontend consumes via `spotlight-files --search <query>`.

use crate::app_search;
use crate::calculator;
use crate::content_index;
use crate::file_search;
use crate::model::{Action, AppEntry, ContentHit, DisplayItem, FileHit};
use std::path::Path;

/// Map a file path to a freedesktop MIME-type icon name based on its
/// extension (and sometimes its basename). The Electron frontend resolves
/// these via GTK3's icon theme to show proper file-type icons.
/// If a specific icon name is not installed, the Python3 fallback in
/// main.cjs degrades gracefully to the category-generic icon.
fn file_icon(path: &str) -> String {
    // Directories: folder icon
    if Path::new(path).is_dir() {
        return "folder".to_string();
    }

    let ext = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // Some important files have no extension (Makefile, Dockerfile, …).
    let basename = Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    match basename.as_str() {
        "dockerfile" | "containerfile" => return "text-x-dockerfile".to_string(),
        "makefile" | "gnumakefile" => return "text-x-makefile".to_string(),
        "cmakelists.txt" => return "text-x-cmake".to_string(),
        "meson.build" | "meson_options.txt" => return "text-x-meson".to_string(),
        "build" | "build.bazel" | "buck" | "workspace" => return "text-x-bazel".to_string(),
        "license" | "licence" | "copying" => return "text-x-generic".to_string(),
        "readme" | "readme.md" | "readme.txt" | "readme.rst" => return "text-x-readme".to_string(),
        "changelog" | "changelog.md" | "changelog.txt" => return "text-x-changelog".to_string(),
        "gemfile" | "rakefile" | "brewfile" | "capfile" | "gemfile.lock" => return "text-x-ruby".to_string(),
        "podfile" | "podfile.lock" => return "text-x-generic".to_string(),
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitconfig" | ".git-blame-ignore-revs"
        | ".git-blame" => return "text-x-generic".to_string(),
        ".dockerignore" | ".npmignore" | ".tokeignore" | ".ignore" => return "text-x-generic".to_string(),
        ".env" | ".env.local" | ".env.production" | ".env.development"
        | ".bashrc" | ".zshrc" | ".bash_profile" | ".profile" | ".bash_logout"
        | ".vimrc" | ".gvimrc" | ".editorconfig" | ".nanorc" | ".tmux.conf"
        | ".git-prompt.sh" => return "text-x-generic".to_string(),
        ".eslintrc" | ".eslintrc.js" | ".eslintrc.json" | ".eslintrc.yml" | ".eslintrc.cjs"
        | ".prettierrc" | ".prettierrc.js" | ".prettierrc.json" | ".prettierrc.yml"
        | ".babelrc" | ".babelrc.js" | ".babelrc.json"
        | ".stylelintrc" | ".stylelintrc.js" | ".stylelintrc.json"
        | ".tern-config" | ".tern-project" => return "application-javascript".to_string(),
        ".eslintignore" | ".prettierignore" | ".stylelintignore" => return "text-x-generic".to_string(),
        "vagrantfile" => return "text-x-ruby".to_string(),
        "procfile" | "procfile.dev" => return "text-x-generic".to_string(),
        "requirements.txt" | "requirements-dev.txt" | "setup.py" | "pyproject.toml"
        | "setup.cfg" | "tox.ini" | "pipfile" | "pipfile.lock" | "poetry.lock"
        | "manifest.in" | "conftest.py" => return "text-x-python".to_string(),
        "package.json" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml"
        | "npm-shrinkwrap.json" | ".npmrc" => return "application-javascript".to_string(),
        "tsconfig.json" | "tsconfig.base.json" | "jsconfig.json" => return "text-typescript".to_string(),
        "webpack.config.js" | "webpack.config.ts" | "webpack.config.cjs"
        | "webpack.config.mjs" | "webpack.dev.js" | "webpack.prod.js"
        | "webpack.common.js" | "rollup.config.js" | "rollup.config.mjs"
        | "vite.config.js" | "vite.config.ts" | "vite.config.mjs"
        | "esbuild.config.js" | "esbuild.config.mjs" | "babel.config.js"
        | "jest.config.js" | "jest.config.ts" | "jest.config.cjs" | "jest.config.mjs"
        | "vitest.config.ts" | "vitest.config.js" | "vitest.config.mjs"
        | "playwright.config.ts" | "cypress.config.js" | "cypress.config.ts" => return "application-javascript".to_string(),
        ".nvmrc" | ".node-version" | ".ruby-version" | ".python-version" | ".tool-versions"
        | ".pythonrc" => return "text-x-generic".to_string(),
        "go.mod" | "go.sum" | "go.work" | "go.work.sum" => return "text-x-go".to_string(),
        "cargo.toml" | "cargo.lock" => return "text-x-rust".to_string(),
        "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts"
        | "gradle.properties" | "gradlew" => return "text-x-groovy".to_string(),
        "pom.xml" => return "text-x-java".to_string(),
        "build.sbt" | "build.sc" | "plugins.sbt" => return "text-x-scala".to_string(),
        "mix.exs" | "mix.lock" => return "text-x-elixir".to_string(),
        "rebar.config" => return "text-x-erlang".to_string(),
        "composer.json" | "composer.lock" => return "text-x-php".to_string(),
        "dub.json" | "dub.sdl" => return "text-x-d".to_string(),
        "shard.yml" | "shard.lock" => return "text-x-crystal".to_string(),
        "packages.config" | "nuget.config" => return "text-x-csharp".to_string(),
        ".gitlab-ci.yml" | ".github" => return "text-x-yaml".to_string(),
        "jenkinsfile" | "jenkinsfile.groovy" => return "text-x-groovy".to_string(),
        "justfile" | "justfile.config" | ".justfile" => return "application-x-shellscript".to_string(),
        _ => {}
    }

    match ext.as_str() {
        // ── Programming languages ──────────────────────────────────
        // Python
        "py" | "pyw" | "pyc" | "pyo" | "pyd" | "pyz" | "pyi" | "pyt" => "text-x-python",
        // C / C++ / Objective-C
        "c" => "text-x-csrc",
        "h" => "text-x-chdr",
        "cpp" | "cc" | "cxx" | "c++" => "text-x-c++src",
        "hpp" | "hh" | "hxx" | "h++" => "text-x-c++hdr",
        "m" => "text-x-objc",
        "mm" => "text-x-objc",
        // Rust / Go / Zig / D / Nim
        "rs" => "text-x-rust",
        "go" => "text-x-go",
        "zig" => "text-x-zig",
        "d" | "di" => "text-x-d",
        "nim" | "nims" | "nimble" => "text-x-nim",
        // JavaScript family
        "js" | "mjs" | "cjs" | "jsx" => "application-javascript",
        "ts" | "tsx" | "mts" | "cts" => "text-typescript",
        "coffee" => "text-x-coffeescript",
        // Java / Kotlin / Scala / Groovy / Clojure
        "java" | "class" | "bsh" => "text-x-java",
        "kt" | "kts" => "text-x-kotlin",
        "scala" | "sbt" => "text-x-scala",
        "groovy" | "gradle" | "gy" => "text-x-groovy",
        "clj" | "cljs" | "cljc" | "cljd" | "edn" => "text-x-clojure",
        // Ruby / PHP / Perl / Python frameworks
        "rb" | "erb" | "rbs" => "text-x-ruby",
        "php" | "php3" | "php4" | "php5" | "phtml" | "pht" => "text-x-php",
        "pl" | "pm" | "pod" | "t" => "text-x-perl",
        // Shell
        "sh" | "bash" | "zsh" | "fish" | "ksh" | "csh" | "tcsh" => "application-x-shellscript",
        // Lua / Tcl / Vim
        "lua" | "luac" => "text-x-lua",
        "tcl" | "tk" => "text-x-tcl",
        "vim" | "viml" => "text-x-vim",
        // Assembly
        "asm" | "s" | "nasm" => "text-x-asm",
        // Apple
        "swift" => "text-x-swift",
        "scpt" | "osa" | "osax" => "text-x-applescript",
        // .NET / VB
        "cs" | "csx" | "csi" => "text-x-csharp",
        "fs" | "fsx" | "fsi" | "fsproj" => "text-x-fsharp",
        "vb" | "vbnet" | "bas" | "frm" | "cls" | "vbs" => "text-x-vb",
        // Pascal / Ada / Fortran
        "pas" | "pp" | "dpr" | "lpr" => "text-x-pascal",
        "ada" | "adb" | "ads" => "text-x-ada",
        "f" | "f90" | "f95" | "f03" | "f08" | "for" | "f77" => "text-x-fortran",
        // Functional
        "hs" | "lhs" | "cabal" => "text-x-haskell",
        "ml" | "mli" => "text-x-ml",
        "elm" => "text-x-elm",
        "ex" | "exs" | "eex" | "heex" | "leex" => "text-x-elixir",
        "erl" | "hrl" => "text-x-erlang",
        "scm" | "sps" | "sls" | "rkt" | "rktl" => "text-x-scheme",
        "lisp" | "lsp" | "cl" => "text-x-lisp",
        "v" | "vh" | "sv" | "svh" => "text-x-verilog",
        "vhd" | "vhdl" => "text-x-vhdl",
        "lean" => "text-x-lean",
        "idr" => "text-x-idris",
        "agda" => "text-x-agda",
        "thy" => "text-x-isabelle",
        "re" | "rei" => "text-x-reason",
        "res" | "resi" => "text-x-rescript",
        "purs" => "text-x-purescript",
        "dhall" => "text-x-dhall",
        "st" | "gst" => "text-x-smalltalk",
        // Scientific / statistical
        "r" | "rmd" | "rdata" | "rds" | "rda" => "text-x-r",
        "jl" => "text-x-julia",
        "dart" => "text-x-dart",
        "cr" => "text-x-crystal",
        "vala" | "vapi" => "text-x-vala",
        "awk" => "application-x-awk",
        // Web components
        "vue" => "text-x-vue",
        "svelte" => "text-x-svelte",
        // Query / schema
        "sql" | "psql" | "pgsql" | "mysql" => "text-x-sql",
        "graphql" | "gql" => "text-x-graphql",
        "proto" | "prototxt" => "text-x-protobuf",
        "thrift" => "text-x-thrift",
        "sol" => "text-x-solidity",
        // Misc / legacy
        "cob" | "cbl" | "ccp" | "cpy" => "text-x-cobol",
        "pl1" | "pli" => "text-x-pl1",
        "fth" | "4th" | "forth" | "fr" => "text-x-forth",
        "pro" | "prolog" => "text-x-prolog",
        "ahk" => "text-x-ahk",
        "au3" => "text-x-autoit",
        "el" | "elc" => "text-x-elisp",
        "gleam" => "text-x-gleam",
        "merl" | "odin" | "jai" | "wren" | "carp" | "roc" | "moon" | "moonbit"
        | "nu" | "janet" | "factor" | "pop" | "dwm" | "red"
        | "rebol" | "r2" | "r3" => "text-x-generic",

        // ── Build / config / project files ────────────────────────
        "mk" => "text-x-makefile",
        "cmake" => "text-x-cmake",
        "lock" => "text-x-lock",
        "diff" | "patch" | "rej" => "text-x-patch",
        "desktop" => "application-x-desktop",
        "theme" | "gtkrc" => "application-x-theme",
        "service" | "target" => "text-x-systemd",
        "toml" => "text-x-toml",
        "ini" | "cfg" | "conf" | "config" | "rc" | "properties" | "env" => "text-x-generic",
        "sln" | "csproj" | "vbproj" => "text-x-csharp",
        "podspec" => "text-x-generic",
        "gemspec" => "text-x-ruby",
        "nupkg" => "text-x-csharp",
        "vsix" => "text-x-generic",

        // ── Web / markup / data formats ────────────────────────────
        "html" | "htm" | "xhtml" | "jhtml" => "text-html",
        "css" | "scss" | "sass" | "less" | "styl" => "text-css",
        "xml" | "xsl" | "xslt" | "dtd" | "plist" => "text-xml",
        "md" | "markdown" | "mkd" | "rst" => "text-markdown",
        "json" | "json5" | "jsonc" | "jsonld" | "ipynb" | "geojson" | "topojson" | "avsc" => "application-json",
        "yaml" | "yml" => "application-x-yaml",
        "csv" => "text-csv",
        "tsv" => "text-x-generic",
        "txt" | "text" => "text-plain",
        "rtf" => "application-rtf",
        "tex" | "latex" | "texi" | "texinfo" => "text-x-tex",
        "log" => "text-x-log",
        "nfo" => "text-x-nfo",
        "man" => "text-x-man",
        "asciidoc" | "adoc" => "text-x-asciidoc",
        "org" | "wiki" | "haml" | "jade" | "pug" => "text-x-generic",

        // ── Documents ──────────────────────────────────────────────
        "pdf" => "application-pdf",
        "doc" | "docx" | "dot" | "dotx" | "wps" => "application-msword",
        "docm" | "dotm" => "application-msword",
        "odt" | "ott" | "fodt" => "application-vnd.oasis.opendocument.text",
        "pages" => "x-office-document",
        "xls" | "xlsx" | "xlt" | "xltx" => "application-vnd.ms-excel",
        "xlsm" | "xlsb" | "xlam" | "xltm" => "application-vnd.ms-excel",
        "ods" | "ots" | "fods" => "application-vnd.oasis.opendocument.spreadsheet",
        "numbers" => "x-office-spreadsheet",
        "ppt" | "pptx" | "pps" | "ppsx" => "application-vnd.ms-powerpoint",
        "pptm" | "potm" | "ppsm" => "application-vnd.ms-powerpoint",
        "odp" | "otp" | "fodp" => "application-vnd.oasis.opendocument.presentation",
        "keynote" => "x-office-presentation",
        "odg" | "otg" | "fodg" => "application-vnd.oasis.opendocument.graphics",
        "epub" => "application-epub+zip",
        "mobi" | "azw" | "azw3" | "azw4" => "application-x-mobipocket-ebook",
        "djvu" | "djv" => "image-vnd.djvu",
        "xps" | "oxps" => "application-xps",
        "cbz" | "cbr" | "cb7" | "cbt" | "cba" => "application-x-cbr",
        "chm" => "application-x-chm",
        "fb2" => "application-x-fictionbook+xml",
        "vsd" | "vsdx" | "vss" | "vst" | "vsdm" | "vssx" | "vstx" | "vssm" | "vstm" => "application-vnd.ms-visio",
        "mpp" | "mpt" | "mpd" => "application-vnd.ms-project",
        "pub" => "application-vnd.ms-publisher",
        "one" | "onetoc2" | "onepkg" => "x-office-document",
        "thmx" => "application-vnd.ms-office",
        // Google Docs shortcuts (use x-office-* icons)
        "gdoc" => "x-office-document",
        "gsheet" => "x-office-spreadsheet",
        "gslides" => "x-office-presentation",
        "gdraw" => "x-office-drawing",
        "gform" => "x-office-document",
        "gsite" => "x-office-document",
        "gscript" => "application-javascript",
        "gmap" => "x-office-document",
        "gpres" => "x-office-presentation",
        // Email / contacts / calendar
        "eml" | "emlx" | "emlxpart" => "message-rfc822",
        "msg" | "oft" | "ost" | "pst" => "x-office-address-book",
        "mbox" | "msf" => "x-office-address-book",
        "vcf" | "vcard" => "x-office-address-book",
        "ics" | "ical" | "icalendar" | "vcs" | "ifb" => "x-office-calendar",
        // Mathematica / Maple / Matlab / scientific
        "nbs" | "wl" | "wls" | "ma" | "mb" | "nb" => "application-x-mathematica",
        "mws" | "mw" => "application-x-maple",
        "mlx" => "application-x-matlab",
        // Stata / SPSS / SAS
        "do" | "dta" | "smcl" | "stsem" => "application-x-stata",
        "por" | "spo" => "application-x-spss",
        "sas" | "sas7bdat" | "sas7bcat" | "sas7bndx" => "application-x-sas",
        // Scientific data formats
        "fits" | "fit" | "fts" => "image-x-generic",
        "feather" | "npy" | "npz" => "application-x-generic-data",
        "pkl" | "pickle" => "application-x-python-bytecode",
        "msgpack" | "bson" | "ubjson" => "application-x-generic-data",
        "hjson" | "cson" | "ison" => "application-json",
        // Bioinformatics / chemistry
        "fasta" | "fa" | "fna" | "faa" | "mpfa" => "text-x-fasta",
        "fastq" | "fq" => "text-x-fasta",
        "sam" | "bam" | "bai" | "cram" | "crai" => "text-x-fasta",
        "bcf" => "text-x-fasta",
        "gbk" | "gb" | "genbank" | "gbf" => "text-x-fasta",
        "gff" | "gff3" | "gtf" | "gtf2" => "text-x-fasta",
        "bed" | "bed12" | "bedpe" | "bedgraph" => "text-x-fasta",
        "mol" | "sdf" | "mol2" | "mdl" => "chemical-x-mdl-molfile",
        "xyz" => "chemical-x-xyz",
        "smi" | "smiles" | "can" => "chemical-x-smiles",
        "inchi" | "inchikey" => "chemical-x-inchi",
        "cif" | "ent" => "chemical-x-pdb",
        "mmcif" | "pdb1" => "chemical-x-pdb",
        "nex" | "nexus" | "nwk" | "newick" => "text-x-fasta",
        "phylip" | "ph" => "text-x-fasta",
        // Music notation
        "mscz" | "mscx" | "msc" | "musescore" => "application-x-musescore",
        "ly" | "lilypond" | "ily" => "text-x-lilypond",
        "musicxml" | "mxl" | "mei" => "application-x-musicxml",
        "mus" | "sib" | "musx" => "application-x-musescore",
        "abc" => "text-x-abc",

        // ── Images ─────────────────────────────────────────────────
        "png" | "jpg" | "jpeg" | "jfif" | "gif" | "bmp" | "webp" | "tiff"
        | "tif" | "ico" | "heic" | "heif" | "avif" | "tga" | "pcx"
        | "pbm" | "pgm" | "ppm" | "pnm" | "xbm" | "xpm" | "exr" | "hdr"
        | "cr2" | "nef" | "arw" | "rw2" | "orf" | "raf" | "dng" | "raw" => "image-x-generic",
        "svg" | "svgz" => "image-svg+xml",
        "psd" => "image-x-psd",
        "eps" | "epsf" | "epsi" => "image-x-eps",
        "ai" => "image-x-adobe-illustrator",
        "kra" => "image-x-krita",

        // ── Audio ──────────────────────────────────────────────────
        "mp3" | "wav" | "flac" | "ogg" | "oga" | "opus" | "aac" | "m4a"
        | "m4b" | "m4p" | "wma" | "aiff" | "aif" | "aifc" | "au" | "snd"
        | "amr" | "gsm" | "ra" | "rm" | "shn" | "ape" | "wv" | "tta"
        | "dsf" | "dff" | "mod" | "s3m" | "xm" | "it" | "mtm" | "ult"
        | "m15" | "mka" | "als" | "caf" | "cda" | "dts" | "vqf" | "voc"
        | "pcm" | "vox" | "oma" | "aa" | "aax" | "m4r"
        | "spx" | "mpc" | "mp+" | "ofr" | "rka" | "rkau" | "pac" | "lpac"
        | "dwd" | "3ga" | "mid" => "audio-x-generic",
        "midi" | "kar" => "audio-midi",

        // ── Video ──────────────────────────────────────────────────
        "mp4" | "m4v" | "mkv" | "avi" | "webm" | "mov" | "qt" | "wmv"
        | "flv" | "f4v" | "mpg" | "mpeg" | "mpe" | "mp2" | "m2v"
        | "vob" | "m2ts" | "3gp" | "3g2" | "ogv"
        | "rmvb" | "asf" | "divx" | "y4m" | "mk3d" | "vro" | "evo"
        | "ram" | "rv" | "smil" | "rvx" | "bdmv"
        | "wtv" | "dv" | "dif" => "video-x-generic",

        // ── Archives / packages ────────────────────────────────────
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "tbz" | "tbz2" | "xz" | "txz"
        | "7z" | "rar" | "lz" | "lzip" | "lzma" | "tlz" | "zst" | "zstd"
        | "cpio" | "ar" | "a" | "lha" | "lzh" | "lzx" | "ace" | "cab"
        | "sit" | "sitx" | "pak" | "lz4" | "lzo" | "br" | "brotli"
        | "zpaq" | "zpaq1" | "gzp" | "bh" | "zlib" | "rz" | "ha"
        | "kex" | "keks" => "application-x-archive",
        "deb" => "application-x-deb",
        "rpm" => "application-x-rpm",
        "apk" => "application-vnd.android.package-archive",
        "appimage" => "application-x-executable",
        "snap" => "application-x-snap",
        "flatpak" | "flatpakref" | "flatpakrepo" => "application-x-flatpak",
        "jar" | "war" | "ear" | "aar" => "application-x-java-archive",
        "whl" => "application-x-python-wheel",
        "egg" => "application-x-python-bytecode",
        "gem" => "application-x-ruby",
        "crate" => "text-x-rust",
        "xpi" => "application-x-xpinstall",
        "crx" => "application-x-chrome-extension",

        // ── Databases / data ───────────────────────────────────────
        "db" | "sqlite" | "sqlite3" | "db3" | "duckdb" => "application-x-sqlite3",
        "accdb" | "mdb" => "application-vnd.ms-access",
        "dbf" => "application-x-dbf",
        "h5" | "hdf5" | "nc4" | "cdf" => "application-x-hdf",
        "mat" => "application-x-matlab",
        "parquet" | "avro" | "arrow" | "orc" => "application-x-generic-data",

        // ── Executables / libraries / object files ────────────────
        "exe" | "bin" | "run" | "app" | "out" => "application-x-executable",
        "so" | "dll" | "dylib" => "application-x-sharedlib",
        "o" | "ko" => "application-x-object",
        "lib" => "application-x-archive",
        "wasm" => "application/wasm",
        "dex" => "application-x-dex",
        "node" => "application-x-nodejs",
        "msi" => "application-x-msi",
        "cmd" | "bat" | "com" | "ps1" | "psm1" => "application-x-executable",
        "xbe" | "xex" => "application-x-executable",

        // ── Fonts ──────────────────────────────────────────────────
        "ttf" | "otf" | "woff" | "woff2" | "eot" | "ttc" | "pfb" | "pfa"
        | "pcf" | "bdf" => "font-x-generic",

        // ── 3D / CAD / model files ─────────────────────────────────
        "blend" | "blend1" => "application-x-blender",
        "fbx" | "gltf" | "glb" | "dae" | "3ds" | "ply" | "iges"
        | "igs" | "step" | "stp" | "dwg" | "dxf" | "skp" | "x3d"
        | "lwo" | "lws" | "c4d" | "u3d" | "3mf" | "amf" | "max" | "xsi"
        | "hip" | "hiplc" | "hipnc" | "otl" | "lxo" | "3dm" | "pov"
        | "povray" | "rib" | "bvh" => "x-model",
        "obj" => "text-x-generic",
        "wrl" | "wrml" | "vrml" => "model-vrml",
        "mtl" => "text-x-generic",
        // FreeCAD / OpenSCAD
        "fcstd" | "f3d" | "fcstd1" => "application-x-freecad",
        "scad" => "application-x-openscad",
        // SolidWorks / Inventor
        "sldprt" | "sldasm" | "slddrw" | "sldftn" | "ipt" | "iam" | "ipn" => "x-model",
        // GIS layer files (ArcGIS)
        "lyr" | "mxd" | "lpk" | "lpkx" => "x-model",

        // ── G-code / CAM ──────────────────────────────────────────
        "gcode" | "gco" | "nc" | "cnc" | "tap" | "ngc" | "g" | "gp" | "nc1"
        | "min" | "k" => "text-x-gcode",

        // ── Disk / VM images ───────────────────────────────────────
        "iso" | "img" | "nrg" | "dmg" | "vmdk" | "vdi" | "qcow2"
        | "vhdx" | "wim" | "ova" | "ovf" | "sparseimage"
        | "sparsebundle" | "mds" | "mdf" | "ccd" | "toast"
        | "vmtk" | "box" | "vmwarevm" => "application-x-cd-image",

        // ── Certificates / keys ────────────────────────────────────
        "pem" | "crt" | "cer" | "cert" | "p12" | "pfx" | "der" | "csr"
        | "key" | "keystore" | "jks" | "ovpn" | "vpn" => "application-x-pem-key",
        "sig" | "asc" | "gpg" | "pgp" => "application-x-pgp-key",

        // ── GIS / mapping / GPS ─────────────────────────────────────
        "shp" | "shx" | "prj" | "qpj" | "sbn" | "sbx"
        | "fbn" | "ain" | "aih" | "ixs" | "mxs"
        | "mif" | "mapinfo" => "application-x-shapefile",
        "kml" | "kmz" => "application-vnd.google-earth.kml+xml",
        "gpx" => "application-x-gpx",
        "osm" => "application-x-osm",
        "gml" | "gfs" => "text-xml",
        "mbtiles" => "application-x-archive",
        "gpkg" => "application-x-sqlite3",
        "grd" | "bil" | "flt" => "image-x-generic",
        "vrt" => "text-x-generic",
        "qgs" | "qgsz" | "qgz" | "qml" => "application-x-qgis",

        // ── Game development ───────────────────────────────────────
        // Godot
        "gd" | "gdscript" | "tscn" | "tres" | "scn" | "escn"
        | "import" | "godot" => "application-x-godot",
        // Unity
        "prefab" | "asset" | "unity" | "anim" | "controller"
        | "shader" | "cginc" | "hlsl" | "shaderlab" | "uxml" | "uss"
        | "meta" | "preset" => "application-x-unity",
        // Unreal Engine
        "uasset" | "umap" | "ublueprint" | "ucurve" | "uparticle"
        | "umaterial" | "umaterialinstance" | "uinterface" | "usf" | "ush"
        => "application-x-unreal",
        // GameMaker / RPG Maker
        "gmx" | "yy" | "yyp" | "yyz" | "yyc" => "application-x-gamemaker",
        "rpgproject" | "rpgmap" | "rpgsave" | "rvdata" | "rvdata2"
        => "application-x-rpgmaker",
        // Source / Quake / idTech engines
        "vmt" | "vtf" | "vpk" | "vvd" | "pk3" | "pk4" | "roq" => "x-model",
        "bsp" | "ani" | "map" => "x-model",

        // ── Infrastructure as Code / config ────────────────────────
        "tf" | "tfvars" | "tfstate" | "tfplan" | "tfbackend" => "text-x-terraform",
        "hcl" | "hcl2" | "nomad" | "nomadvars" => "text-x-hcl",
        "nix" => "application-x-nix",
        "star" | "bzl" | "bazel" | "sky" => "text-x-bazel",
        "epp" => "text-x-puppet",
        "dockerfile" | "containerfile" => "text-x-dockerfile",
        "k8s" | "kube" | "kustomization" | "helm" | "chart" | "helmignore"
        => "application-x-yaml",
        "pkt" | "hurl" | "http" | "rest" => "text-x-generic",
        "pcap" | "pcapng" | "cap" | "capx" => "application-x-pcap",
        "jsonnet" | "libsonnet" => "text-x-generic",

        // ── Mobile / iOS dev ────────────────────────────────────────
        "xib" | "storyboard" | "nib" | "storyboardc" => "application-x-storyboard",
        "xcassets" | "xcdatamodeld" | "xcmappingmodel" | "xcodeproj"
        | "xcworkspace" | "xcconfig" | "xcscheme" | "xctest" | "xctestplan"
        => "application-x-xcode",
        "aidl" | "arsc" | "smali" | "odex" | "vdex" => "application-x-android",
        "aab" => "application-vnd.android.package-archive",

        // ── Subtitles ──────────────────────────────────────────────
        "srt" | "ssa" | "ass" | "sub" | "idx" | "sup" | "vtt" | "webvtt"
        | "scc" | "sbv" | "ttml" | "dfxp" | "lrc" => "text-x-subtitle",

        // ── Game saves ─────────────────────────────────────────────
        "save" | "sav" | "gsl" | "gci" | "srm" | "srm2" => "application-x-game-save",

        // ── Notes apps ─────────────────────────────────────────────
        "enex" | "note" | "notebook" => "x-office-document",
        "obsidian" => "text-markdown",
        "roam" | "roamedit" | "notion" | "notion-cache" => "text-x-generic",
        "bear" | "bearbak" => "text-markdown",

        // ── Other common types ────────────────────────────────────
        "torrent" => "application-x-bittorrent",
        "part" | "partial" | "crdownload" => "application-x-partial",
        "tmp" | "temp" | "swp" | "swo" | "bak" | "old" | "orig" | "pid" => "text-x-generic",
        "lnk" => "application-x-mswinurl",
        "url" | "webloc" => "text-x-uri",
        "po" | "pot" | "mo" | "gmo" => "text-x-gettext-translation",
        "qmd" => "text-x-generic",
        "scope" | "locale" | "locales" | "manifest" => "text-x-generic",
        "directory" => "application-x-desktop",

        // ── Default: generic text file ────────────────────────────
        _ => "text-x-generic",
    }
    .to_string()
}

/// Combine apps + file hits for a query into one display list.
///
/// Ordering:
///   1. Calculator result (if the query looks like math)
///   2. Fuzzy-matched applications (best score first, then alphabetical)
///   3. plocate file hits
///   4. Full-text content matches (with snippet)
///
/// The final list is capped at 30 rows for a snappy UI.
pub fn build_results(
    query: &str,
    apps: &[AppEntry],
    files: &[FileHit],
    content: &[ContentHit],
) -> Vec<DisplayItem> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();             // nothing to show for empty input
    }

    let mut items = Vec::new();

    // ── Calculator ────────────────────────────────────────────────
    // If the query looks like math, evaluate it and offer "= result".
    if calculator::looks_like_math(q) {
        if let Some(r) = calculator::eval(q) {
            let f = calculator::format_result(r);
            items.push(DisplayItem {
                icon: "accessories-calculator".to_string(),
                title: format!("= {}", f),                         // e.g. "= 8"
                subtitle: "Calculator  ·  Enter to copy".to_string(),
                action: Action::CopyResult(f),                     // Enter → clipboard
                is_content: false,
            });
        }
    }

    // ── Applications ─────────────────────────────────────────────
    // Fuzzy-match the query against each app's display name; keep pairs of
    // (score, app) so we can rank by match quality then by name.
    let mut app_matches: Vec<(i32, &AppEntry)> = apps
        .iter()
        .filter_map(|a| fuzzy(q, &a.name).map(|s| (s, a)))
        .collect();
    // Higher score first; ties broken alphabetically by app name.
    app_matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    // Show at most 8 apps to leave room for file + content results.
    for (_, a) in app_matches.iter().take(8) {
        items.push(DisplayItem {
            // Fall back to a generic icon if the .desktop had no Icon=.
            icon: a
                .icon
                .clone()
                .unwrap_or_else(|| "application-x-executable".to_string()),
            title: a.name.clone(),
            subtitle: "Application".to_string(),
            action: Action::LaunchApp(a.app_id.clone()),
            is_content: false,
        });
    }

    // ── Files ────────────────────────────────────────────────────
    // Each file hit becomes one row: the base name as the title, the parent
    // directory as the subtitle, and `xdg-open <path>` as the action.
    for f in files.iter().take(8) {
        let name = Path::new(&f.path)
            .file_name()                                  // last path component
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| f.path.clone());
        let dir = Path::new(&f.path)
            .parent()                                     // everything but the name
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        items.push(DisplayItem {
            icon: file_icon(&f.path),
            title: name,
            subtitle: dir,
            action: Action::OpenFile(f.path.clone()),
            is_content: false,
        });
    }

    // ── Content matches ─────────────────────────────────────────
    // Files whose *contents* match the query (via the Tantivy index). The
    // snippet from the matching region is shown as the subtitle so the user
    // can see the context without opening the file.
    for c in content.iter().take(12) {
        let name = Path::new(&c.path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| c.path.clone());
        items.push(DisplayItem {
            icon: file_icon(&c.path),
            title: name,
            subtitle: c.snippet.clone(),
            action: Action::OpenFile(c.path.clone()),
            is_content: true,
        });
    }

    // Hard cap so very large result sets never bog down the UI.
    items.truncate(30);
    items
}

/// Convenience wrapper: load apps + run file + content search, then build results.
pub fn search(query: &str) -> Vec<DisplayItem> {
    let apps = app_search::load_apps();                 // parse .desktop files
    // Only hit plocate / content index for queries of >= 2 chars.
    let (file_hits, content_hits) = if query.trim().len() >= 2 {
        (
            file_search::search_files(query, 100),
            content_index::search_content(query, 20),
        )
    } else {
        (Vec::new(), Vec::new())
    };
    build_results(query, &apps, &file_hits, &content_hits)
}

/// Escape a string for safe inclusion inside a JSON string literal.
/// We hand-roll JSON (no serde) to keep the binary tiny, so we must escape.
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),     // quote → \"
            '\\' => out.push_str("\\\\"),   // backslash → \\
            '\n' => out.push_str("\\n"),     // newline
            '\t' => out.push_str("\\t"),     // tab
            '\r' => out.push_str("\\r"),     // carriage return
            // Other control characters → \uXXXX.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),                // normal character, copied verbatim
        }
    }
    out
}

/// Map an `Action` to the string the frontend expects in `action_type`.
fn action_type(a: &Action) -> &'static str {
    match a {
        Action::LaunchApp(_) => "launch_app",
        Action::OpenFile(_) => "open_file",
        Action::CopyResult(_) => "copy",
    }
}

/// The payload string for an action — the app id, file path, or text to copy.
fn action_data(a: &Action) -> &str {
    match a {
        Action::LaunchApp(id) => id,
        Action::OpenFile(p) => p,
        Action::CopyResult(t) => t,
    }
}

/// Produce the JSON the Electron frontend renders. Shape:
///   {"items":[{"title":..,"subtitle":..,"icon":..,"action_type":..,"action_data":..}, ...]}
pub fn search_json(query: &str) -> String {
    let items = search(query);
    // Serialize each item as its own JSON object.
    let parts: Vec<String> = items
        .iter()
        .map(|i| {
            format!(
                r#"{{"title":"{}","subtitle":"{}","icon":"{}","action_type":"{}","action_data":"{}","is_content":{}}}"#,
                escape_json(&i.title),
                escape_json(&i.subtitle),
                escape_json(&i.icon),
                action_type(&i.action),
                escape_json(action_data(&i.action)),
                i.is_content,
            )
        })
        .collect();
    // Join all items into the outer object.
    format!(r#"{{"items":[{}]}}"#, parts.join(","))
}

/// Fuzzy match: does `query` appear in `target` as a subsequence (case-insensitive)?
/// Returns `Some(score)` if it matches, where higher score = better match.
/// Boundary starts of words and consecutive matches score higher.
fn fuzzy(query: &str, target: &str) -> Option<i32> {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = target.to_lowercase().chars().collect();
    if q.is_empty() {
        return Some(0);                 // empty query "matches" everything, score 0
    }
    let mut qi = 0usize;                // cursor into the query
    let mut score = 0i32;
    let mut prev_matched = false;       // was the previous target char a match?
    for (i, &tc) in t.iter().enumerate() {
        if qi < q.len() && tc == q[qi] {
            // A word boundary (start, or after a non-alphanumeric char) is a strong match.
            let boundary = i == 0 || !t[i - 1].is_alphanumeric();
            if boundary {
                score += 10;            // matched at the start of a word
            } else if prev_matched {
                score += 5;             // continuation of a run of matches
            }
            score += 1;                 // base score for any match
            qi += 1;                    // consume one query character
            prev_matched = true;
        } else {
            prev_matched = false;       // broke a run
        }
    }
    // Only a match if the entire query was consumed.
    if qi == q.len() {
        Some(score)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_basic() {
        // Subsequence "fir" matches "Firefox".
        assert!(fuzzy("fir", "Firefox").is_some());
        // "xyz" is not a subsequence of "Firefox".
        assert!(fuzzy("xyz", "Firefox").is_none());
    }

    #[test]
    fn json_empty_query() {
        let j = search_json("");
        assert!(j.contains("\"items\":[]"));          // empty query → no items
    }

    #[test]
    fn json_has_structure() {
        let j = search_json("firefox");
        // Must be a well-formed wrapper object.
        assert!(j.starts_with("{\"items\":["));
        assert!(j.ends_with("]}"));
    }

    #[test]
    fn json_escapes_quotes() {
        // Embedded quotes must be backslash-escaped.
        let j = escape_json(r#"he said "hi""#);
        assert!(j.contains(r#"\"hi\""#));
    }
}
