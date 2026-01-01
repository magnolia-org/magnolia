import { writable } from 'svelte/store';

export const modalStore = writable({
  activeModal: null, // 'cache', 'about', or null
});

export const openModal = (modalName) => {
  modalStore.update(s => ({ ...s, activeModal: modalName }));
};

export const closeModal = () => {
  modalStore.update(s => ({ ...s, activeModal: null }));
};
