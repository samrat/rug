## rug, a Git implementation in Rust

[![CircleCI](https://circleci.com/gh/samrat/rug.svg?style=svg)](https://circleci.com/gh/samrat/rug)

This is my implementation of *Jit*, from James Coglan's book
[*Building Git*](https://shop.jcoglan.com/building-git/).


### Usage

Build the `rug` binary and add it to your PATH:

```sh
$ cargo build
$ export PATH=/path/to/rug/target/debug:$PATH
```

Switch to the directory you want to track using `rug`:

```
$ mkdir /tmp/rug-test && cd /tmp/rug-test
$ mkdir -p foo/bar

$ echo "hello" > hello.txt
$ echo "world" > foo/bar/world.txt
```

Finally, initialize a Git repo and create a commit:

```
$ rug init
$ rug add .

# Currently, this waits for your input. Type in your commit message
and hit Ctrl+D
$ rug commit
```

You should now be able to use Git to view the commit you just created:

```
git show
```


### Other supported commands

```
rug status
rug status --porcelain
```

```
rug diff
rug diff --cached
```

```
rug branch foo HEAD~5
```
