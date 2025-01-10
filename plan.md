3 places where information is:
- the csv of metadata from your library
- the csv that maps metadata to spotify song ids
- the spotify playlist

## subcommands

### map
takes a csv of songs from your library and a (potentially non-existant) mapping csv
any library song missing in the mapping is added. A spotify song id is attempted to be found, but if not it needs to be added manually afterwards
any mapping missing from the library is removed

### check
takes a mapping csv, marks defunct spotify song id's with a flag

### upload
takes a mapping, updates the spotify playlist to reflect the contents of the mapping by only adding new songs and removing missing ones
