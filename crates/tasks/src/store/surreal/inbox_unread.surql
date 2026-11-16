LET $wanted = array::complement(
    object::values(object::from_entries(array::flatten([$candidates, $acked]))),
    [NULL]
);
