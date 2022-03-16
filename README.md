# Fontgardener

An experimental tool to manage large font projects, with the following goals:

1. Suitable for large font projects with multiple scripts moving at different speeds and with different scopes
2. Trying hard to avoid Git merge conflicts by reducing their surface area
3. Easier diffing for human eyes by structuring data in tables where it makes sense
4. Avoiding data duplication where reasonably possible

A format aimed at font gardeners, or engineers, rather than font designers and design applications.

## Status

Very bare bones and experimental. Can currently only handle glyphs.

## Example Usage

### Creating a new Fontgarden

```shell
$ fontgardener new MyFont.fontgarden
```

### Importing Glyphs into Sets

Prepare a text file with the glyphs (one glyph name per line) you want to import into a set, per set. Example Latin.txt:

```txt
A
B
C
C.alt
```

Example default.txt:

```txt
one
two
BASE
```

Then run:

```shell
$ fontgardener import MyFont.fontgarden Latin.txt --set-name Latin MyFont-Regular.ufo MyFont-Italic.ufo
```

### Exporting Back into UFOs

To export whole sets:

```shell
$ fontgardener export MyFont.fontgarden --set-names default --set-names Latin --output-dir some/dir/for/output
```

To export glyphs by list, make another one-line-per-name file like above and run:

```shell
$ fontgardener export MyFont.fontgarden --glyph-names-file Export.txt --output-dir some/dir/for/output
```

To limit the sources to just what you want (i.e. only work on the Regular and nothing else):

```shell
$ fontgardener export MyFont.fontgarden --glyph-names-file Export.txt --source-names Regular --output-dir some/dir/for/output
```

Repeat the switch to select more sources, e.g. `--source-names Regular --source-names Italic`.
