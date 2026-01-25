pub const ADMIN: &str = "Admin";
pub const OPERATOR: &str = "Operator";
pub const VIEWER: &str = "Viewer";

pub fn is_admin(role: &str) -> bool {
    role == ADMIN
}

pub fn is_operator(role: &str) -> bool {
    role == OPERATOR
}

pub fn can_operate(role: &str) -> bool {
    role == ADMIN || role == OPERATOR
}
