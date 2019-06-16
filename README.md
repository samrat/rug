## rug, a Git implementation in Rust

This is my implementation of *Jit*, from James Coglan's book *Building
Git*.


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
