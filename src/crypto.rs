/// OpenAPI `ComputerConfigBo.cpuId` is documented as a hardware MD5.
pub fn machine_code(cpu: &str, disk: &str, board: &str) -> String {
    let blob = format!("{cpu}|{disk}|{board}");
    format!("{:x}", md5::compute(blob.as_bytes()))
}
