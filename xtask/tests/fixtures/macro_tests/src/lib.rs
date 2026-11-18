pub mod model {
    #[cfg(test)]
    mod tests {
        macro_rules! pair {
            ($first:ident, $second:ident) => {
                #[test]
                fn $first() {}

                #[test]
                fn $second() {}
            };
        }

        pair!(left, right);
    }
}
