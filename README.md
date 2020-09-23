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

### Gotchas with the `rug` repo

I use `rug` as the version-control system for the `rug`
source-code. However, because all commands are not implemented yet,
I've been using `git` for eg. pushing to Github.

This means sometimes you might have to hackily modify files in `.git`
to bring it back into a state that `rug` finds acceptable. Here are
some ways in which things can break:

1. Updating `master` after pulling from `origin`
   `rug pull` does not currently work.

   After running `git fetch origin`, copy the SHA from
   `.git/refs/remotes/origin/master` into `.git/refs/heads/master`:

   ```shell
   cp .git/refs/remotes/origin/master .git/refs/heads/master
   ```

2. `rug` doesn't understand packed objects

    Copy the packed object outside `.git` and unpack it:

    ```
    mkdir temp
    mv .git/objects/pack/pack-ab7ec7453bc7444032731b68f2c1fe06279bd017.pack temp/
    git unpack-objects < temp/pack-ab7ec7453bc7444032731b68f2c1fe06279bd017.pack
    ```
