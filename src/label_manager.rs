use crate::error::BDKCliError;
use bip329::{ExportError, Label, LabelRef, Labels, ParseError};
use std::fs::File;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct LabelManager {
    labels: Labels,
    file_path: PathBuf,
}

impl LabelManager {
    pub fn new(wallet_data_dir: &Path) -> Result<Self, BDKCliError> {
        let file_path = wallet_data_dir.join("labels.jsonl");
        log::debug!("Label file path: {}", file_path.display());

        let labels = match Labels::try_from_file(&file_path) {
            Ok(loaded_labels) => {
                log::info!("Loaded {} labels from {}", loaded_labels.len(), file_path.display());
                loaded_labels
            }
            Err(ParseError::FileReadError(io_err)) if io_err.kind() == ErrorKind::NotFound => {
                log::info!("Label file {} not found, starting with empty labels.", file_path.display());
                Labels::default()
            }
            Err(e) => {
                return Err(BDKCliError::LabelError(format!(
                    "Failed to load labels from {}: {}",
                    file_path.display(),
                    e
                )));
            }
        };
        Ok(Self { labels, file_path })
    }

    pub fn set_label(&mut self, label_to_set: Label) {
        self.labels.set_label(label_to_set); // bip329::Labels handles add or update
    }

    pub fn get_label_by_ref(&self, item_ref: &LabelRef) -> Option<&Label> {
        self.labels.iter().find(|l| l.ref_() == *item_ref)
    }

    pub fn get_label_text_by_ref(&self, item_ref: &LabelRef) -> Option<String> {
        self.get_label_by_ref(item_ref)
            .and_then(|l| l.label().map(|s| s.to_string()))
    }

    pub fn get_all_labels(&self) -> &Labels {
        &self.labels
    }

    pub fn import_labels(&mut self, new_labels: Labels) -> usize {
        let mut count = 0;
        for label_to_import in new_labels.into_iter() { // Consumes new_labels by iterating
            self.set_label(label_to_import); // set_label clones internally if needed by bip329 crate
            count += 1;
        }
        count
    }

    pub fn save(&self) -> Result<(), BDKCliError> {
        if self.labels.is_empty() && !self.file_path.exists() {
            log::debug!("No labels to save and file doesn't exist. Skipping save.");
            return Ok(());
        }

        let parent_dir = self.file_path.parent().ok_or_else(|| {
            BDKCliError::LabelError(format!(
                "Cannot get parent directory for label file: {}",
                self.file_path.display()
            ))
        })?;

        let temp_file_name = format!(
            ".labels.jsonl.tmp.{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        let temp_path = parent_dir.join(temp_file_name);

        log::debug!("Atomically saving labels to {} via temporary file {}", self.file_path.display(), temp_path.display());

        // Create scope for temp_file_handle to ensure it's closed before rename
        {
            let mut temp_file_handle = File::create(&temp_path).map_err(|e| {
                BDKCliError::LabelError(format!(
                    "Failed to create temporary label file {}: {}",
                    temp_path.display(),
                    e
                ))
            })?;

            // Use the export_to_writer method from bip329 if available, or serialize and write line by line.
            // Assuming bip329::Labels has an export method that returns String or writes to a writer.
            // The `export_to_file` method in `bip329` likely handles this well.
            // If `export_to_file` itself is not atomic, we do it here.
            // For now, let's assume `bip329::Labels::export_to_file` is used directly on temp_path
        }
        // If export_to_file is not directly on a handle, but takes a Path:
        self.labels.export_to_file(&temp_path).map_err(|e: ExportError| { // Explicitly type ExportError
            BDKCliError::LabelError(format!(
                "Failed to export labels to temporary file {}: {}",
                temp_path.display(),
                e
            ))
        })?;


        std::fs::rename(&temp_path, &self.file_path).map_err(|e| {
            // Attempt to clean up temp file if rename fails
            let _ = std::fs::remove_file(&temp_path);
            BDKCliError::LabelError(format!(
                "Failed to rename temporary label file {} to {}: {}",
                temp_path.display(),
                self.file_path.display(),
                e
            ))
        })?;

        log::info!("Labels successfully saved to {}", self.file_path.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bip329::{AddressRecord, TransactionRecord};
    use bdk_wallet::bitcoin::{Address, Network, Txid};
    use std::str::FromStr;
    use tempfile::tempdir;

    fn dummy_addr() -> Address {
        Address::from_str("bcrt1q7cyrfmzx308z9pa82r4hjgmw8v7sns57t68p5r")
            .unwrap()
            .assume_checked()
    }
    fn dummy_txid() -> Txid {
        Txid::from_str("f4184fc596403b9d638783cf57adfe4c75c605f6356fbc91338530e9831e9e16").unwrap()
    }

    #[test]
    fn test_label_manager_new_and_save_empty() {
        let dir = tempdir().unwrap();
        let lm = LabelManager::new(dir.path()).unwrap();
        assert_eq!(lm.get_all_labels().len(), 0);
        lm.save().unwrap(); // Should not error, might not create file if empty
        assert!(!dir.path().join("labels.jsonl").exists() || std::fs::read_to_string(dir.path().join("labels.jsonl")).unwrap().is_empty());
    }

    #[test]
    fn test_label_manager_set_get_save_load() {
        let dir = tempdir().unwrap();
        let mut lm = LabelManager::new(dir.path()).unwrap();

        let addr_label = Label::Address(AddressRecord {
            ref_: dummy_addr().into_unchecked(),
            label: Some("Test Address Label".to_string()),
        });
        let tx_label = Label::Tx(TransactionRecord {
            ref_: dummy_txid(),
            label: Some("Test TX Label".to_string()),
            origin: None,
        });

        lm.set_label(addr_label.clone());
        lm.set_label(tx_label.clone());

        assert_eq!(lm.get_all_labels().len(), 2);
        assert_eq!(
            lm.get_label_text_by_ref(&addr_label.ref_()),
            Some("Test Address Label".to_string())
        );
        assert_eq!(
            lm.get_label_text_by_ref(&tx_label.ref_()),
            Some("Test TX Label".to_string())
        );

        lm.save().unwrap();
        assert!(dir.path().join("labels.jsonl").exists());

        // Load into new manager
        let lm2 = LabelManager::new(dir.path()).unwrap();
        assert_eq!(lm2.get_all_labels().len(), 2);
        assert_eq!(
            lm2.get_label_text_by_ref(&addr_label.ref_()),
            Some("Test Address Label".to_string())
        );
    }

    #[test]
    fn test_label_manager_import() {
        let dir = tempdir().unwrap();
        let mut lm = LabelManager::new(dir.path()).unwrap();

        let mut new_labels = Labels::default();
        new_labels.add_label_unchecked(Label::Address(AddressRecord {
            ref_: dummy_addr().into_unchecked(),
            label: Some("Imported Addr Label".to_string()),
        }));
         new_labels.add_label_unchecked(Label::Tx(TransactionRecord {
            ref_: dummy_txid(),
            label: Some("Imported TX Label".to_string()),
            origin: None,
        }));

        let import_count = lm.import_labels(new_labels);
        assert_eq!(import_count, 2);
        assert_eq!(lm.get_all_labels().len(), 2);
        assert_eq!(
            lm.get_label_text_by_ref(&LabelRef::Address(dummy_addr().into_unchecked())),
            Some("Imported Addr Label".to_string())
        );

        // Test overwrite
        let mut newer_labels = Labels::default();
        newer_labels.add_label_unchecked(Label::Address(AddressRecord {
             ref_: dummy_addr().into_unchecked(),
             label: Some("Overwritten Addr Label".to_string()),
        }));
        let import_count_overwrite = lm.import_labels(newer_labels);
        assert_eq!(import_count_overwrite, 1);
        assert_eq!(lm.get_all_labels().len(), 2);
        assert_eq!(
            lm.get_label_text_by_ref(&LabelRef::Address(dummy_addr().into_unchecked())),
            Some("Overwritten Addr Label".to_string())
        );
    }
}
