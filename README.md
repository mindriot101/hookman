# Hookman

This project helps manage git hooks.

## Usage

Create a configuration file in your project directory:

```toml
[hookman]

[[hooks]]
name = "Test"
command = "pytest"
stage = "pre-push"

[[hooks]]
name = "Generate hooks"
command = "ctags --tag-relative-yes -Rf.git/tags.$$ $(git ls-files)"
background = true
stage = "post-commit"

[[hooks]]
name = "Lint"
command = "pylint"
# stage defaults to pre-commit
```

Then run `hookman install`. When you change your configuration, run `hookman install` again.