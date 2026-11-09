//! Reading the optional formats: Parquet, Arrow IPC and SQLite.
//!
//! Each test writes a small invoice table in the format and checks that it
//! loads like the same CSV file.

#![cfg(any(feature = "parquet", feature = "arrow", feature = "sqlite"))]

#[cfg(any(feature = "parquet", feature = "arrow"))]
mod arrow_data {
    use arrow_array::{ArrayRef, Decimal128Array, Int64Array, RecordBatch, StringArray};
    use std::sync::Arc;

    /// product (text), qty (integer), price (decimal with 2 places, one
    /// missing) and total.
    pub fn batch() -> RecordBatch {
        let price = Decimal128Array::from(vec![Some(250), Some(1210), None])
            .with_precision_and_scale(10, 2)
            .unwrap();
        let total = Decimal128Array::from(vec![Some(500), Some(3630), Some(0)])
            .with_precision_and_scale(10, 2)
            .unwrap();
        RecordBatch::try_from_iter([
            (
                "product",
                Arc::new(StringArray::from(vec!["pen", "book", "gift"])) as ArrayRef,
            ),
            ("qty", Arc::new(Int64Array::from(vec![2, 3, 1])) as ArrayRef),
            ("price", Arc::new(price) as ArrayRef),
            ("total", Arc::new(total) as ArrayRef),
        ])
        .unwrap()
    }

    pub fn check(loaded: &regrex_io::Loaded) {
        assert_eq!(loaded.header, ["product", "qty", "price", "total"]);
        assert_eq!(loaded.skipped, ["product"]);
        assert_eq!(loaded.table.names, ["qty", "price", "total"]);
        assert_eq!(loaded.rows[1], ["book", "3", "12.10", "36.30"]);
        // Decimals keep their scale; the missing price is NaN.
        assert_eq!(loaded.table.decimals[1], Some(2));
        assert_eq!(loaded.table.columns[1][1], 12.1);
        assert!(loaded.table.columns[1][2].is_nan());
    }
}

#[cfg(feature = "parquet")]
#[test]
fn reads_parquet() {
    use parquet::arrow::ArrowWriter;
    let batch = arrow_data::batch();
    let mut bytes = Vec::new();
    let mut writer = ArrowWriter::try_new(&mut bytes, batch.schema(), None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
    arrow_data::check(&regrex_io::load_bytes("invoices.parquet", &bytes, None).unwrap());
    // Recognised by its content too.
    arrow_data::check(&regrex_io::load_bytes("download", &bytes, None).unwrap());
}

#[cfg(feature = "arrow")]
#[test]
fn reads_arrow_files_and_streams() {
    use arrow_ipc::writer::{FileWriter, StreamWriter};
    let batch = arrow_data::batch();
    let mut file = Vec::new();
    let mut writer = FileWriter::try_new(&mut file, &batch.schema()).unwrap();
    writer.write(&batch).unwrap();
    writer.finish().unwrap();
    drop(writer);
    arrow_data::check(&regrex_io::load_bytes("invoices.feather", &file, None).unwrap());

    let mut stream = Vec::new();
    let mut writer = StreamWriter::try_new(&mut stream, &batch.schema()).unwrap();
    writer.write(&batch).unwrap();
    writer.finish().unwrap();
    drop(writer);
    arrow_data::check(&regrex_io::load_bytes("invoices.arrows", &stream, None).unwrap());
}

#[cfg(feature = "sqlite")]
#[test]
fn reads_sqlite_tables_like_sheets() {
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE customers (id INTEGER, name TEXT);
         INSERT INTO customers VALUES (1, 'Anna');
         CREATE TABLE \"order lines\" (product TEXT, qty INTEGER, price REAL, total REAL, note BLOB);
         INSERT INTO \"order lines\" VALUES ('pen', 2, 2.5, 5.0, x'00'), ('book', 3, 12.1, 36.3, NULL), ('gift', 1, NULL, 0, NULL);",
    )
    .unwrap();
    let bytes = conn.serialize("main").unwrap().to_vec();

    let first = regrex_io::load_bytes("shop.db", &bytes, None).unwrap();
    assert_eq!(first.sheets, ["customers", "order lines"]);
    assert_eq!(first.header, ["id", "name"]);

    let lines = regrex_io::load_bytes("shop.sqlite", &bytes, Some("order lines")).unwrap();
    assert_eq!(lines.table.names, ["qty", "price", "total"]);
    assert_eq!(lines.rows[1][..4], ["book", "3", "12.1", "36.3"]);
    assert!(lines.table.columns[1][2].is_nan());

    let missing = regrex_io::load_bytes("shop.sqlite", &bytes, Some("nope")).unwrap_err();
    assert!(missing.0.contains("customers, order lines"), "{missing}");
}
