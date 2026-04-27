/// 文件读取工具集成测试
use std::fs;
use std::path::Path;
use tempfile::tempdir;

#[tokio::test]
async fn read_file_from_temp_directory() {
    // 创建临时目录
    let temp_dir = tempdir().expect("Failed to create tempdir");
    let test_file_path = temp_dir.path().join("test.txt");

    // 写入测试内容
    let content = "Test file content for integration test\n";
    fs::write(&test_file_path, content).expect("Failed to write test file");

    // 验证文件存在
    assert!(test_file_path.exists());

    // 读取文件验证内容
    let read_content = fs::read_to_string(&test_file_path).expect("Failed to read test file");
    assert_eq!(read_content, content);
}

#[tokio::test]
async fn read_nonexistent_file_returns_error() {
    let temp_dir = tempdir().expect("Failed to create tempdir");
    let nonexistent = temp_dir.path().join("nonexistent.txt");

    // 验证文件不存在
    assert!(!nonexistent.exists());

    // 尝试读取应返回错误
    let result = fs::read_to_string(&nonexistent);
    assert!(result.is_err());
}

#[tokio::test]
async fn read_binary_file() {
    let temp_dir = tempdir().expect("Failed to create tempdir");
    let binary_path = temp_dir.path().join("binary.bin");

    // 写入二进制内容
    let data = b"\x00\x01\x02\x03\x04\x05";
    fs::write(&binary_path, data).expect("Failed to write binary file");

    let read_data = fs::read(&binary_path).expect("Failed to read binary file");
    assert_eq!(read_data.len(), 6);
    assert_eq!(read_data, data.to_vec());
}

#[tokio::test]
async fn file_size_calculation() {
    let temp_dir = tempdir().expect("Failed to create tempdir");
    let file_path = temp_dir.path().join("size_test.txt");

    let content = String::from_utf8(vec![b'a'; 1024]).unwrap();
    fs::write(&file_path, &content).expect("Failed to write file");

    let metadata = fs::metadata(&file_path).expect("Failed to get metadata");
    assert_eq!(metadata.len(), 1024);
}

#[tokio::test]
async fn large_file_handling() {
    let temp_dir = tempdir().expect("Failed to create tempdir");
    let large_file = temp_dir.path().join("large.txt");

    // 创建一个 ~10MB 的文件
    let data = String::from_utf8(vec![b'x'; 10 * 1024 * 1024]).unwrap();
    fs::write(&large_file, &data).expect("Failed to write large file");

    let metadata = fs::metadata(&large_file).expect("Failed to get metadata");
    assert!(metadata.len() >= 10 * 1024 * 1024);
}
