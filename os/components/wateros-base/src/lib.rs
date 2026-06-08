#![no_std]
//! 引导阶段与内核各处共享的薄基础类型：物理/虚拟地址包装、hart ID、单核同步容器等。
//!
//! 与具体板级、内存布局或子系统容量相关的数值常量见独立包
//! `wateros-base-config`。本 crate 只提供无策略的基础类型与小型工具，不维护
//! 配置常量，避免同一数值在多个基础包中出现“双真相”。
pub mod addr;
pub mod boot;
pub mod cpu;
pub mod sync;
