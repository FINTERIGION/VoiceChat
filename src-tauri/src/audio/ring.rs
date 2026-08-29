use ringbuf::HeapRb;

pub type Producer = ringbuf::HeapProd<f32>;
pub type Consumer = ringbuf::HeapCons<f32>;

pub fn spsc(capacity: usize) -> (Producer, Consumer) {
    use ringbuf::traits::Split;
    HeapRb::<f32>::new(capacity).split()
}
