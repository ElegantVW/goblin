use goblin::cli::{replace_account, save_account, secret_id};
use goblin::config::{load_accounts, purelymail_preset};
use goblin::error::Error;
use goblin::paths;
use goblin::secrets;

#[test]
fn mend_rename_keeps_secret_when_password_none() {
    let root = tempfile::tempdir().unwrap();
    paths::with_goblin_home(Some(root.path()), || {
        let acc = purelymail_preset("work", "Ada <ada@x>", "ada@x");
        save_account(acc.clone(), "hunter2", true).unwrap();
        assert_eq!(secrets::load_password(&secret_id(&acc)).unwrap(), "hunter2");

        let renamed = purelymail_preset("home", "Ada <ada@x>", "ada@x");
        replace_account("work", renamed.clone(), None).unwrap();

        let file = load_accounts(&paths::accounts_file()).unwrap();
        assert_eq!(file.default, "home");
        assert!(file.account("work").is_err());
        assert_eq!(
            secrets::load_password(&secret_id(&renamed)).unwrap(),
            "hunter2"
        );
    });
}

#[test]
fn mend_email_change_moves_secret() {
    let root = tempfile::tempdir().unwrap();
    paths::with_goblin_home(Some(root.path()), || {
        let acc = purelymail_preset("work", "Ada <ada@x>", "ada@x");
        save_account(acc.clone(), "hunter2", true).unwrap();

        let updated = purelymail_preset("work", "Ada <ada@y>", "ada@y");
        replace_account("work", updated.clone(), None).unwrap();

        let file = load_accounts(&paths::accounts_file()).unwrap();
        assert_eq!(file.default, "work");
        assert_eq!(file.account("work").unwrap().imap.user, "ada@y");
        assert!(secrets::load_password(&secret_id(&acc)).is_err());
        assert_eq!(
            secrets::load_password(&secret_id(&updated)).unwrap(),
            "hunter2"
        );
    });
}

#[test]
fn mend_name_collision_errors() {
    let root = tempfile::tempdir().unwrap();
    paths::with_goblin_home(Some(root.path()), || {
        let work = purelymail_preset("work", "Ada <ada@x>", "ada@x");
        let home = purelymail_preset("home", "Ada <ada@y>", "ada@y");
        save_account(work.clone(), "hunter2", true).unwrap();
        save_account(home.clone(), "hunter3", false).unwrap();

        let collide = purelymail_preset("home", "Ada <ada@x>", "ada@x");
        match replace_account("work", collide, None) {
            Err(Error::Hint { msg, next }) => {
                assert!(msg.contains("already watches"), "{msg}");
                assert_eq!(next, "pick another name");
            }
            other => panic!("{other:?}"),
        }

        let file = load_accounts(&paths::accounts_file()).unwrap();
        assert_eq!(file.default, "work");
        assert!(file.account("work").is_ok());
        assert!(file.account("home").is_ok());
        assert_eq!(
            secrets::load_password(&secret_id(&work)).unwrap(),
            "hunter2"
        );
        assert_eq!(
            secrets::load_password(&secret_id(&home)).unwrap(),
            "hunter3"
        );
    });
}

#[test]
fn mend_missing_goblin_errors() {
    let root = tempfile::tempdir().unwrap();
    paths::with_goblin_home(Some(root.path()), || {
        let acc = purelymail_preset("work", "Ada <ada@x>", "ada@x");
        match replace_account("work", acc.clone(), None) {
            Err(Error::Hint { msg, next }) => {
                assert!(msg.contains("no goblins yet"), "{msg}");
                assert_eq!(next, "goblin summon");
            }
            other => panic!("{other:?}"),
        }

        save_account(acc, "hunter2", true).unwrap();
        let ghost = purelymail_preset("ghost", "Ada <ada@x>", "ada@x");
        match replace_account("ghost", ghost, None) {
            Err(Error::Hint { msg, next }) => {
                assert!(msg.contains("no goblin named"), "{msg}");
                assert_eq!(next, "goblin who");
            }
            other => panic!("{other:?}"),
        }
    });
}
