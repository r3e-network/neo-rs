#[cfg(feature = "neofs-grpc")]
#[allow(
    clippy::doc_overindented_list_items,
    clippy::doc_lazy_continuation,
    clippy::large_enum_variant,
    clippy::enum_variant_names,
    clippy::module_inception,
    dead_code
)]
pub(super) mod neofs_proto {
    pub mod neo {
        pub mod fs {
            pub mod v2 {
                pub mod object {
                    tonic::include_proto!("neo.fs.v2.object");
                }
                pub mod refs {
                    tonic::include_proto!("neo.fs.v2.refs");
                }
                pub mod session {
                    tonic::include_proto!("neo.fs.v2.session");
                }
                pub mod acl {
                    tonic::include_proto!("neo.fs.v2.acl");
                }
                pub mod status {
                    tonic::include_proto!("neo.fs.v2.status");
                }
            }
        }
    }
}

#[cfg(feature = "neofs-grpc")]
pub(super) use self::neofs_proto::neo::fs::v2 as neofs_v2;
