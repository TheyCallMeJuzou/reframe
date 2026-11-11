use super::*;

#[test]
fn shows_folders_under_the_home_folder_from_tilde() {
    assert_eq!(
        home_relative("C:\\Users\\Ann\\projects\\shop", "C:\\Users\\Ann", true),
        "~\\projects\\shop"
    );
    assert_eq!(
        home_relative("c:\\users\\ann\\projects", "C:\\Users\\Ann\\", true),
        "~\\projects"
    );
    assert_eq!(
        home_relative("/home/ann/projects/shop", "/home/ann", false),
        "~/projects/shop"
    );
    assert_eq!(home_relative("/home/ann", "/home/ann", false), "~");
}

#[test]
fn leaves_other_folders_alone_also_ones_whose_name_only_starts_like_the_home_folder() {
    assert_eq!(
        home_relative("D:\\work\\shop", "C:\\Users\\Ann", true),
        "D:\\work\\shop"
    );
    assert_eq!(
        home_relative("/home/anna/shop", "/home/ann", false),
        "/home/anna/shop"
    );
    // case matters there
    assert_eq!(
        home_relative("/home/Ann/shop", "/home/ann", false),
        "/home/Ann/shop"
    );
    assert_eq!(home_relative("/srv/shop", "", false), "/srv/shop");
}
